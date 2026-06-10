//! `devsplit-core` — motor do devsplit, independente de GUI.
//!
//! Reverse-proxy local transparente que faz split por path-prefix: prefixos
//! escolhidos vao p/ servicos locais, o resto faz passthrough p/ o gateway
//! remoto real (IP pinado + cert validado por SNI). Inclui geracao de CA/cert
//! local, edicao idempotente do arquivo hosts, DNS direto (anti-loop) e parsing
//! do `devsplit.yaml`.
//!
//! A casca Tauri (em `app/`) embute este crate e expoe a UI; este crate nao
//! depende de Tauri nem de webkit, entao compila e testa em qualquer lugar.

pub mod config;
pub mod dns;
pub mod hostsfile;
pub mod proxy;
pub mod tlsca;
pub mod types;

pub use types::{
    Decision, PassthroughTarget, ProxyConfig, Route, RouteTable, TrafficEvent, Upstream,
};
