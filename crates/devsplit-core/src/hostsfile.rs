//! Edicao idempotente do arquivo hosts: bloco demarcado por marcadores, backup
//! preservado e escrita atomica (tempfile no mesmo diretorio + persist).

use std::fs;
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

/// Marcador usado nos comentarios de bloco.
pub const MARKER: &str = "devsplit";

/// Linha inicial do bloco gerenciado pelo devsplit.
const BEGIN: &str = "# >>> devsplit BEGIN >>>";
/// Linha final do bloco gerenciado pelo devsplit.
const END: &str = "# <<< devsplit END <<<";

/// Caminho do arquivo hosts do SO.
/// - Windows: `%SystemRoot%\System32\drivers\etc\hosts` (via env `SystemRoot`).
/// - Demais: `/etc/hosts`.
pub fn hosts_path() -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        PathBuf::from(root)
            .join("System32")
            .join("drivers")
            .join("etc")
            .join("hosts")
    } else {
        PathBuf::from("/etc/hosts")
    }
}

/// `true` se o conteudo ja contem o bloco demarcado do devsplit.
pub fn has_block(content: &str) -> bool {
    content.lines().any(|l| l.trim() == BEGIN)
}

/// PURA: remove o bloco demarcado (BEGIN..END inclusive) do conteudo.
/// Preserva o restante; garante newline final quando ha conteudo.
pub fn render_without_block(existing: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut in_block = false;
    for line in existing.lines() {
        let t = line.trim();
        if t == BEGIN {
            in_block = true;
            continue;
        }
        if t == END {
            in_block = false;
            continue;
        }
        if !in_block {
            kept.push(line);
        }
    }
    let mut s = kept.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

/// PURA: produz o conteudo final = (conteudo SEM bloco antigo) + bloco novo.
/// Cada entrada vira `IP\thostname` em sua propria linha (1 nome por linha).
/// Idempotente: `render(render(x, e), e) == render(x, e)`.
pub fn render(existing: &str, entries: &[(IpAddr, String)]) -> String {
    let hosts: Vec<&str> = entries.iter().map(|(_, h)| h.as_str()).collect();
    let base = strip_block_and_hosts(existing, &hosts);
    let mut out = String::new();
    let trimmed = base.trim_end_matches('\n');
    if !trimmed.is_empty() {
        out.push_str(trimmed);
        out.push('\n');
    }
    out.push_str(BEGIN);
    out.push('\n');
    for (ip, host) in entries {
        out.push_str(&ip.to_string());
        out.push('\t');
        out.push_str(host);
        out.push('\n');
    }
    out.push_str(END);
    out.push('\n');
    out
}

/// PURA: remove o bloco do devsplit E qualquer linha "solta" que aponte um dos
/// `hosts` para loopback (limpa duplicatas de setups antigos). Usado no desligar
/// p/ o FQDN voltar a resolver normalmente (stage acessivel de novo).
pub fn render_revert(existing: &str, hosts: &[&str]) -> String {
    strip_block_and_hosts(existing, hosts)
}

/// Remove o bloco demarcado e, quando `hosts` nao e vazio, qualquer linha solta
/// que mapeie um desses hostnames para um endereco de loopback.
fn strip_block_and_hosts(existing: &str, hosts: &[&str]) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut in_block = false;
    for line in existing.lines() {
        let t = line.trim();
        if t == BEGIN {
            in_block = true;
            continue;
        }
        if t == END {
            in_block = false;
            continue;
        }
        if in_block {
            continue;
        }
        if !hosts.is_empty() {
            if let Some((ip, line_hosts)) = parse_host_line(line) {
                if ip_is_loopback(ip) && line_hosts.iter().any(|h| hosts.contains(h)) {
                    continue;
                }
            }
        }
        kept.push(line);
    }
    let mut s = kept.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

/// "IP host1 host2  # comentario" -> (IP, [hosts]); None p/ comentarios/vazias.
fn parse_host_line(line: &str) -> Option<(&str, Vec<&str>)> {
    let l = line.split('#').next().unwrap_or("").trim();
    if l.is_empty() {
        return None;
    }
    let mut it = l.split_whitespace();
    let ip = it.next()?;
    let hosts: Vec<&str> = it.collect();
    if hosts.is_empty() {
        None
    } else {
        Some((ip, hosts))
    }
}

fn ip_is_loopback(ip: &str) -> bool {
    ip.parse::<IpAddr>().map(|a| a.is_loopback()).unwrap_or(false)
}

/// Caminho do backup: `<hosts>.devsplit.bak` (irmao do arquivo hosts).
fn backup_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".devsplit.bak");
    PathBuf::from(os)
}

/// Escreve `content` em `path` de forma atomica: arquivo temporario no MESMO
/// diretorio (p/ garantir rename atomico no mesmo filesystem) + rename.
fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(".devsplit-hosts.{}.{nanos}.tmp", std::process::id()));

    // Bloco isolado: garante que o handle feche (flush implicito) antes do rename.
    {
        let mut f = fs::File::create(&tmp).with_context(|| format!("criar temp {tmp:?}"))?;
        f.write_all(content.as_bytes())
            .with_context(|| format!("escrever temp {tmp:?}"))?;
        f.flush().context("flush do temp")?;
        f.sync_all().context("sync do temp")?;
    }

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("persistir hosts em {path:?}"));
    }
    Ok(())
}

/// Aplica o bloco do devsplit ao arquivo hosts: cria backup `.devsplit.bak`
/// (preservando o ORIGINAL na primeira vez) e reescreve atomicamente.
pub fn apply(path: &Path, entries: &[(IpAddr, String)]) -> Result<()> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let bak = backup_path(path);
    if path.exists() && !bak.exists() {
        fs::copy(path, &bak).with_context(|| format!("criar backup {bak:?}"))?;
    }
    let new = render(&existing, entries);
    atomic_write(path, &new)
}

/// Remove o bloco do devsplit do arquivo hosts (escrita atomica).
pub fn remove(path: &Path) -> Result<()> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let new = render_without_block(&existing);
    atomic_write(path, &new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn entries() -> Vec<(IpAddr, String)> {
        vec![
            (IpAddr::V4(Ipv4Addr::LOCALHOST), "api.stage.acme.com".to_string()),
            (IpAddr::V4(Ipv4Addr::LOCALHOST), "app.stage.acme.com".to_string()),
        ]
    }

    #[test]
    fn render_is_idempotent() {
        let base = "127.0.0.1\tlocalhost\n";
        let e = entries();
        let once = render(base, &e);
        let twice = render(&once, &e);
        assert_eq!(once, twice, "render deve ser idempotente");
    }

    #[test]
    fn render_one_name_per_line() {
        let e = entries();
        let out = render("", &e);
        assert!(out.contains("127.0.0.1\tapi.stage.acme.com\n"));
        assert!(out.contains("127.0.0.1\tapp.stage.acme.com\n"));
        // cada hostname em sua propria linha
        let body_lines: Vec<&str> = out
            .lines()
            .filter(|l| l.contains("acme.com"))
            .collect();
        assert_eq!(body_lines.len(), 2);
        for l in body_lines {
            assert_eq!(l.matches("acme.com").count(), 1, "1 nome por linha");
        }
    }

    #[test]
    fn has_block_reflects_marker() {
        assert!(!has_block("127.0.0.1 localhost\n"));
        let out = render("", &entries());
        assert!(has_block(&out));
    }

    #[test]
    fn render_without_block_strips_only_block() {
        let base = "127.0.0.1\tlocalhost\n# minha linha\n";
        let with = render(base, &entries());
        let stripped = render_without_block(&with);
        assert!(!has_block(&stripped));
        assert!(stripped.contains("127.0.0.1\tlocalhost"));
        assert!(stripped.contains("# minha linha"));
        assert!(!stripped.contains("acme.com"));
    }

    #[test]
    fn apply_then_remove_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        fs::write(&path, "127.0.0.1\tlocalhost\n").unwrap();

        apply(&path, &entries()).unwrap();
        let after_apply = fs::read_to_string(&path).unwrap();
        assert!(has_block(&after_apply), "bloco presente apos apply");
        assert!(after_apply.contains("api.stage.acme.com"));
        // backup do original criado
        assert!(backup_path(&path).exists());

        remove(&path).unwrap();
        let after_remove = fs::read_to_string(&path).unwrap();
        assert!(!has_block(&after_remove), "bloco ausente apos remove");
        assert!(after_remove.contains("127.0.0.1\tlocalhost"));
    }

    #[test]
    fn apply_is_idempotent_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        fs::write(&path, "127.0.0.1\tlocalhost\n").unwrap();

        apply(&path, &entries()).unwrap();
        let first = fs::read_to_string(&path).unwrap();
        apply(&path, &entries()).unwrap();
        let second = fs::read_to_string(&path).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn render_dedups_stale_lines_and_revert_clears_fqdn() {
        use std::net::Ipv4Addr;
        let host = "api.hml.example.com";
        // setup antigo deixou linhas soltas apontando o FQDN p/ loopback
        let existing = format!(
            "127.0.0.1\tlocalhost\n127.0.0.1 {host}\n127.0.0.1 {host}\n203.0.113.1 outro.com\n"
        );
        let entries = vec![(IpAddr::V4(Ipv4Addr::LOCALHOST), host.to_string())];
        let rendered = render(&existing, &entries);
        assert_eq!(rendered.matches(host).count(), 1, "uma unica entrada do FQDN (no bloco)");
        assert!(has_block(&rendered));
        assert!(rendered.contains("127.0.0.1\tlocalhost"), "preserva localhost");
        assert!(rendered.contains("203.0.113.1 outro.com"), "preserva hosts nao-loopback");
        // revert remove o bloco E as soltas do FQDN -> some de vez
        let reverted = render_revert(&rendered, &[host]);
        assert_eq!(reverted.matches(host).count(), 0, "FQDN some totalmente no revert");
        assert!(reverted.contains("127.0.0.1\tlocalhost"));
        assert!(reverted.contains("203.0.113.1 outro.com"));
    }
}
