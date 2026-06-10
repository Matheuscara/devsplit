//! Resolucao DNS DIRETA (ignora /etc/hosts) p/ achar o IP real do gateway
//! remoto sem cair no loop de interceptacao local.

use std::net::IpAddr;

use anyhow::{anyhow, Result};
use hickory_resolver::config::{NameServerConfigGroup, ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;

/// Resolve `fqdn` via DNS DIRETO (Cloudflare 1.1.1.1 + Google 8.8.8.8),
/// IGNORANDO o arquivo hosts (anti-loop). Retorna o primeiro IPv4 encontrado;
/// se nao houver nenhum, o primeiro IPv6.
pub async fn resolve_direct(fqdn: &str) -> Result<IpAddr> {
    // Cloudflare + Google como resolvers diretos (UDP/TCP :53).
    let mut servers = NameServerConfigGroup::cloudflare();
    servers.merge(NameServerConfigGroup::google());
    let config = ResolverConfig::from_parts(None, vec![], servers);

    // Anti-loop: NUNCA consultar o arquivo hosts (onde o devsplit aponta o FQDN
    // p/ 127.0.0.1). Precisamos do IP REAL do gateway remoto.
    let mut opts = ResolverOpts::default();
    opts.use_hosts_file = false;

    let resolver = TokioAsyncResolver::tokio(config, opts);
    let lookup = resolver
        .lookup_ip(fqdn)
        .await
        .map_err(|e| anyhow!("falha ao resolver `{fqdn}` via DNS direto: {e}"))?;

    let mut first_v6: Option<IpAddr> = None;
    for ip in lookup.iter() {
        match ip {
            IpAddr::V4(_) => return Ok(ip),
            IpAddr::V6(_) if first_v6.is_none() => first_v6 = Some(ip),
            IpAddr::V6(_) => {}
        }
    }
    first_v6.ok_or_else(|| anyhow!("nenhum IP retornado p/ `{fqdn}`"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Teste de rede real: requer acesso a internet, por isso `#[ignore]` (CI
    /// roda sem rede). Rode com `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn resolves_public_domain() {
        let ip = resolve_direct("one.one.one.one").await.unwrap();
        // 1.1.1.1 / 1.0.0.1 (ou os AAAA correspondentes).
        assert!(!ip.is_unspecified(), "ip resolvido nao deve ser 0.0.0.0/::");
    }
}
