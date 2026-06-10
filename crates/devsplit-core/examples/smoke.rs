//! Smoke test REAL do motor: sobe o proxy numa porta alta (sem privilegio),
//! resolve o IP real do stage via DNS direto, e faz passthrough validando o
//! cert remoto. Uso:
//!   cargo run -p devsplit-core --example smoke -- <fqdn> [porta_local]
//! Depois, de outro terminal:
//!   curl --resolve <fqdn>:8443:127.0.0.1 --cacert /tmp/devsplit-smoke-ca/rootCA.pem https://<fqdn>:8443/

use std::sync::Arc;

use arc_swap::ArcSwap;
use devsplit_core::proxy;
use devsplit_core::tlsca;
use devsplit_core::dns;
use devsplit_core::types::{PassthroughTarget, ProxyConfig, RouteTable};

#[tokio::main]
async fn main() {
    let fqdn = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "api.hml.gateway.acme.com".to_string());
    let port: u16 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8443);

    let ip = dns::resolve_direct(&fqdn).await.expect("DNS direto falhou");
    eprintln!("[smoke] {fqdn} -> IP real {ip} (via DNS direto, ignorando /etc/hosts)");

    let dir = std::path::PathBuf::from("/tmp/devsplit-smoke-ca");
    std::fs::create_dir_all(&dir).unwrap();
    let ca = tlsca::ensure_ca(&dir).expect("ensure_ca");
    let leaf = tlsca::issue_leaf(&ca, &[fqdn.clone()], 825).expect("issue_leaf");
    let server_config = tlsca::build_server_config(&leaf).expect("server_config");

    let cfg = ProxyConfig {
        listen_host: "127.0.0.1".into(),
        listen_port: port,
        intercept_host: fqdn.clone(),
        routes: RouteTable::new(vec![]), // sem rota local => tudo cai no passthrough
        passthrough: PassthroughTarget {
            sni: fqdn.clone(),
            resolve_host: fqdn.clone(),
            fixed_ip: Some(ip),
            port: 443,
            verify: true,
        },
        extra_hosts: Vec::new(),
    };
    let shared = Arc::new(ArcSwap::from_pointee(cfg));
    let listen = format!("127.0.0.1:{port}").parse().unwrap();
    let _handle = proxy::serve(listen, server_config, shared, None)
        .await
        .expect("serve");

    println!("READY ca=/tmp/devsplit-smoke-ca/rootCA.pem listen=127.0.0.1:{port}");
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
}
