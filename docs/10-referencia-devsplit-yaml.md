# 10 — Referência do `devsplit.yaml`

> **Spec canônica do schema de config.** Campo a campo, com tipos, defaults e o que o
> núcleo realmente consome (`crates/devsplit-core/src/config.rs`). O arquivo é
> **commitável na raiz do repo** do time: um `git pull` distribui a config (modelo
> Localias/DDEV). O exemplo vivo está em `examples/devsplit.yaml`.

---

## 1. Visão geral e exemplo

A UI gerencia tudo, mas lê/escreve este YAML. O parsing é serde
(`serde_yaml::from_str`); `to_proxy_config(cfg, profile, environment)` resolve perfis e
ambientes para o snapshot runtime `ProxyConfig`.

```yaml
version: 1

upstream:
  host: api.hml.gateway.acme.com    # FQDN de stage → /etc/hosts (127.0.0.1)
  kind: stage                             # stage|qa  (prod recusado por padrão)
  passthrough:
    resolve: api.hml.gateway.acme.com  # re-resolvido via DNS DIRETO (ignora /etc/hosts)
    # address: 192.0.2.10:443           # OU pin de IP fixo (dispensa o DNS)
    sni: api.hml.gateway.acme.com      # ServerName no upstream → cert remoto VALIDADO
    verify: true                             # NUNCA false em stage real

tls:
  provider: mkcert        # mkcert (shell-out) | self (rcgen em-processo)
  leaf_max_days: 825      # teto cross-platform (limite do macOS)

profiles:
  default:
    routes:
      - prefix: /transporte
        target: http://127.0.0.1:3000
        also: [/socket.io]        # socket.io junto do app (mesmo upstream)
  transporte-e-auth:
    extends: default
    routes:
      - prefix: /auth
        target: http://127.0.0.1:3001
  full-local:
    extends: transporte-e-auth
    routes:
      - prefix: /financeiro
        target: http://127.0.0.1:3002

environments:
  qa:
    upstream:
      host: api.qa.gateway.acme.com
      passthrough:
        resolve: api.qa.gateway.acme.com
        sni: api.qa.gateway.acme.com
        verify: true

cors:
  enabled: true
  allow_origins: ["https://app.hml.acme.com"]
  allow_origins_regex: ['^https?://(localhost|127\.0\.0\.1)(:\d+)?$']
  allow_credentials: true

defaults:
  listen: "0.0.0.0:443"
```

---

## 2. Campos da raiz (`DevsplitConfig`)

| Campo | Tipo | Obrigatório | Default | Nota |
|---|---|---|---|---|
| `version` | `u32` | não | `0` | Versão do schema. |
| `upstream` | `UpstreamSpec` | **sim** | — | Gateway primário a interceptar (§3). |
| `extra_upstreams` | `[UpstreamSpec]` | não | `null` | Hosts adicionais interceptados ao mesmo tempo — **passthrough-only** (multi-host, `01` §7 → `ProxyConfig.extra_hosts`). |
| `tls` | `TlsSpec` | não | `null` | §5. |
| `profiles` | `map<string, ProfileSpec>` | não | `{}` | §4. |
| `environments` | `map<string, EnvironmentSpec>` | não | `{}` | §6. |
| `cors` | `CorsSpec` | não | `null` | §7. |
| `defaults` | `DefaultsSpec` | não | `null` | §8. |

---

## 3. `upstream` (`UpstreamSpec`)

| Campo | Tipo | Obrigatório | Nota |
|---|---|---|---|
| `host` | `string` | **sim** | FQDN interceptado → vai p/ `/etc/hosts` (127.0.0.1) e p/ o SAN do cert. Vira `ProxyConfig.intercept_host`. |
| `passthrough` | `PassthroughSpec` | **sim** | Destino do catch-all (abaixo). |
| `kind` | `string` | não | `stage`/`qa`. Informativo; prod deve ser recusado por design (`11` §4). |

### 3.1 `passthrough` (`PassthroughSpec`)

| Campo | Tipo | Obrigatório | Default | Nota |
|---|---|---|---|---|
| `resolve` | `string` | **sim** | — | FQDN resolvido via **DNS direto** (ignora `/etc/hosts`) p/ achar o IP real. |
| `address` | `string` (`ip:port`) | não | `null` | **Pin de IP fixo**; quando presente dispensa a resolução DNS (vira `fixed_ip`). |
| `sni` | `string` | **sim** | — | `ServerName` no upstream **e** alvo da validação do cert remoto. |
| `verify` | `bool` | não | **`true`** | Validar o cert remoto. **Nunca `false`** em stage real. |

> A porta do passthrough vem de `address` (se dado) ou default **443**.

---

## 4. `profiles` (`ProfileSpec`) e rotas

Cada perfil é um conjunto nomeado de rotas locais.

| Campo | Tipo | Nota |
|---|---|---|
| `extends` | `string?` | Herda as rotas de outro perfil (resolução **recursiva**, com detecção de ciclo). |
| `routes` | `[RouteSpec]` | Rotas locais deste perfil. |

### 4.1 `RouteSpec`

| Campo | Tipo | Obrigatório | Nota |
|---|---|---|---|
| `prefix` | `string` | **sim** | Prefixo de path (ex.: `/transporte`). Mais específico vence (`01` §3). |
| `target` | `string` | **sim** | `http://127.0.0.1:3000` — o **esquema é ignorado**, só host:porta importam. |
| `also` | `[string]` | não | Prefixos extras para o **mesmo** target (ex.: `[/socket.io]`). Cada item vira uma `Route` própria apontando ao mesmo `Upstream::Local`. |

A UI lista os perfis com `default` primeiro, depois alfabético (`profile_names`).

---

## 5. `tls` (`TlsSpec`)

| Campo | Tipo | Default | Nota |
|---|---|---|---|
| `provider` | `string?` | — | `mkcert` (shell-out p/ instalar o trust) ou `self` (rcgen em-processo). |
| `leaf_max_days` | `u32?` | — | Teto da validade do leaf. A casca usa **825** (`LEAF_MAX_DAYS`), teto do macOS. |

---

## 6. `environments` (`EnvironmentSpec`)

Mapa `nome → { upstream: UpstreamSpec }`. Selecionado na UI; **substitui** o `upstream`
base pelo do ambiente (mesmo schema de §3). Útil para alternar stage↔qa sem trocar de
arquivo.

---

## 7. `cors` (`CorsSpec`)

| Campo | Tipo | Default | Consumido? |
|---|---|---|---|
| `enabled` | `bool` | `false` | — |
| `allow_origins` | `[string]` | `[]` | — |
| `allow_credentials` | `bool` | `false` | — |

> `[VERIFICAR]` **Divergência real:** o `examples/devsplit.yaml` inclui
> `allow_origins_regex`, mas o `CorsSpec` **não tem** esse campo — o serde o **ignora**
> silenciosamente. Além disso, o motor de proxy hoje **espelha a `Origin`** da request em
> vez de aplicar a allowlist de `allow_origins` (`02` §2.3). Ou seja: o bloco `cors` é,
> hoje, mais documental que efetivo no roteamento. Reconciliar (aplicar a allowlist no
> motor, ou adicionar `allow_origins_regex` ao `CorsSpec`) é trabalho aberto.

---

## 8. `defaults` (`DefaultsSpec`)

| Campo | Tipo | Default | Nota |
|---|---|---|---|
| `listen` | `string` (`host:port`) | `0.0.0.0:443` | Endereço de bind → `ProxyConfig.listen_host`/`listen_port`. |

---

## 9. Estado runtime (NÃO vai no YAML)

Rotas editadas pela UI, perfil ativo, IP pinado e hash do cert ficam **separados** (estado
runtime por perfil, no diretório de config do app) — **nunca** no `devsplit.yaml`
commitado. Isso preserva os comentários e o `extends` que você edita à mão (`02` §3.6).
