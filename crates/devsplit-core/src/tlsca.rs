//! Geracao de CA/leaf local (rcgen) + orquestracao do mkcert p/ instalar o
//! trust no sistema. A CA assina certificados leaf p/ os FQDNs interceptados.

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose,
};
use rustls::ServerConfig;

/// Autoridade certificadora local persistida em disco.
pub struct Ca {
    pub cert_pem: String,
    pub key_pem: String,
    pub dir: PathBuf,
}

/// Certificado leaf (folha) emitido pela CA local.
pub struct Leaf {
    pub cert_pem: String,
    pub key_pem: String,
}

const CA_CERT_FILE: &str = "rootCA.pem";
const CA_KEY_FILE: &str = "rootCA-key.pem";

/// Parametros DETERMINISTICOS da CA. Usados tanto na geracao quanto na
/// reconstrucao em memoria (p/ assinar leaves): o que importa p/ a cadeia e o
/// DN + a chave, ambos estaveis entre execucoes.
fn ca_params() -> Result<CertificateParams> {
    let mut params = CertificateParams::new(Vec::<String>::new())
        .context("montar parametros da CA")?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "devsplit local CA");
    dn.push(DnType::OrganizationName, "devsplit");
    params.distinguished_name = dn;

    let now = SystemTime::now();
    params.not_before = (now - Duration::from_secs(86_400)).into();
    // ~100 anos: a CA local nao deve expirar durante o uso.
    params.not_after = (now + Duration::from_secs(100 * 365 * 86_400)).into();
    Ok(params)
}

/// Garante a CA local em `caroot`: gera com rcgen se ausente (persiste
/// `rootCA.pem` + `rootCA-key.pem`); recarrega do disco se ja existir.
pub fn ensure_ca(caroot: &Path) -> Result<Ca> {
    fs::create_dir_all(caroot).with_context(|| format!("criar caroot {caroot:?}"))?;
    let cert_path = caroot.join(CA_CERT_FILE);
    let key_path = caroot.join(CA_KEY_FILE);

    if cert_path.exists() && key_path.exists() {
        let cert_pem = fs::read_to_string(&cert_path)
            .with_context(|| format!("ler {cert_path:?}"))?;
        let key_pem = fs::read_to_string(&key_path)
            .with_context(|| format!("ler {key_path:?}"))?;
        return Ok(Ca {
            cert_pem,
            key_pem,
            dir: caroot.to_path_buf(),
        });
    }

    let key = KeyPair::generate().context("gerar chave da CA")?;
    let params = ca_params()?;
    let cert = params.self_signed(&key).context("auto-assinar CA")?;
    let cert_pem = cert.pem();
    let key_pem = key.serialize_pem();

    fs::write(&cert_path, &cert_pem).with_context(|| format!("escrever {cert_path:?}"))?;
    fs::write(&key_path, &key_pem).with_context(|| format!("escrever {key_path:?}"))?;

    Ok(Ca {
        cert_pem,
        key_pem,
        dir: caroot.to_path_buf(),
    })
}

/// Emite um leaf assinado pela CA p/ `fqdns`. SAN inclui: cada FQDN, o wildcard
/// `*.dominio` do dominio pai (quando aplicavel) e os IPs `127.0.0.1` + `::1`.
/// A validade fica limitada a `max_days`.
pub fn issue_leaf(ca: &Ca, fqdns: &[String], max_days: u32) -> Result<Leaf> {
    // Reconstroi o emissor a partir da chave persistida. DN + chave sao
    // estaveis (ver `ca_params`), entao leaves chained validam contra o
    // rootCA.pem em disco.
    let ca_key = KeyPair::from_pem(&ca.key_pem).context("carregar chave da CA")?;
    let ca_cert = ca_params()?
        .self_signed(&ca_key)
        .context("reconstruir cert da CA")?;

    let mut sans: Vec<String> = Vec::new();
    for f in fqdns {
        if !sans.contains(f) {
            sans.push(f.clone());
        }
        // wildcard do dominio pai: `host.dom.tld` -> `*.dom.tld`
        if let Some((_, rest)) = f.split_once('.') {
            if rest.contains('.') {
                let wc = format!("*.{rest}");
                if !sans.contains(&wc) {
                    sans.push(wc);
                }
            }
        }
    }
    for ip in ["127.0.0.1", "::1"] {
        let ip = ip.to_string();
        if !sans.contains(&ip) {
            sans.push(ip);
        }
    }

    let mut params = CertificateParams::new(sans).context("montar parametros do leaf")?;
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    if let Some(first) = fqdns.first() {
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, first.clone());
        params.distinguished_name = dn;
    }

    let now = SystemTime::now();
    params.not_before = (now - Duration::from_secs(3600)).into();
    params.not_after = (now + Duration::from_secs(max_days as u64 * 86_400)).into();

    let leaf_key = KeyPair::generate().context("gerar chave do leaf")?;
    let leaf_cert = params
        .signed_by(&leaf_key, &ca_cert, &ca_key)
        .context("assinar leaf com a CA")?;

    Ok(Leaf {
        cert_pem: leaf_cert.pem(),
        key_pem: leaf_key.serialize_pem(),
    })
}

/// Constroi um `ServerConfig` rustls a partir do leaf (cadeia + chave).
pub fn build_server_config(leaf: &Leaf) -> Result<Arc<ServerConfig>> {
    crate::proxy::ensure_crypto_provider();

    let mut cert_rd = BufReader::new(leaf.cert_pem.as_bytes());
    let certs = rustls_pemfile::certs(&mut cert_rd)
        .collect::<Result<Vec<_>, _>>()
        .context("parsear certificados do leaf")?;
    if certs.is_empty() {
        return Err(anyhow!("nenhum certificado no PEM do leaf"));
    }

    let mut key_rd = BufReader::new(leaf.key_pem.as_bytes());
    let key = rustls_pemfile::private_key(&mut key_rd)
        .context("parsear chave do leaf")?
        .ok_or_else(|| anyhow!("nenhuma chave privada no PEM do leaf"))?;

    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("montar ServerConfig com o leaf")?;
    Ok(Arc::new(cfg))
}

/// Instala a CA do mkcert no trust store do sistema via `mkcert -install`,
/// usando `caroot` como `CAROOT`. Erro claro com instrucao se o binario faltar.
pub fn mkcert_install(caroot: &Path) -> Result<()> {
    use std::process::Command;
    let status = Command::new("mkcert")
        .env("CAROOT", caroot)
        .arg("-install")
        .status()
        .map_err(|e| {
            anyhow!(
                "nao foi possivel executar `mkcert` ({e}). Instale o mkcert: \
                 https://github.com/FiloSottile/mkcert#installation"
            )
        })?;
    if !status.success() {
        return Err(anyhow!("`mkcert -install` falhou ({status})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_ca_generates_and_reloads() {
        let dir = tempfile::tempdir().unwrap();
        let ca1 = ensure_ca(dir.path()).unwrap();
        assert!(dir.path().join(CA_CERT_FILE).exists());
        assert!(dir.path().join(CA_KEY_FILE).exists());
        assert!(ca1.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(ca1.key_pem.contains("PRIVATE KEY"));

        // Reload: deve devolver o MESMO material persistido.
        let ca2 = ensure_ca(dir.path()).unwrap();
        assert_eq!(ca1.cert_pem, ca2.cert_pem);
        assert_eq!(ca1.key_pem, ca2.key_pem);
    }

    #[test]
    fn issue_leaf_and_build_server_config() {
        let dir = tempfile::tempdir().unwrap();
        let ca = ensure_ca(dir.path()).unwrap();
        let leaf = issue_leaf(&ca, &["api.stage.acme.com".to_string()], 825).unwrap();

        assert!(leaf.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(!leaf.cert_pem.is_empty());
        assert!(leaf.key_pem.contains("PRIVATE KEY"));

        let cfg = build_server_config(&leaf);
        assert!(cfg.is_ok(), "build_server_config deve retornar Ok: {cfg:?}");
    }
}
