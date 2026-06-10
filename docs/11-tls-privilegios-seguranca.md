# 11 — TLS & confiança, privilégios e segurança

> Spec do subsistema mais delicado: gerar/confiar um cert local, elevar privilégio sem
> incomodar, e **não** transformar o app num vetor de ataque. Canônico para
> redação de headers, política anti-prod e a chave da CA. Espelha `tlsca.rs`,
> a elevação de `lib.rs` e `redact_headers` de `proxy/mod.rs`.

---

## 1. TLS local (rcgen) — CA e leaf

`tlsca.rs` gera e persiste uma **CA local** em `caroot` (diretório de config do app):

- `ensure_ca(caroot)` — se ausente, gera com **rcgen** e persiste `rootCA.pem` +
  `rootCA-key.pem`; se presente, recarrega do disco. Os parâmetros da CA são
  **determinísticos** (DN + chave estáveis entre execuções) para a cadeia bater na
  reconstrução em memória.
- `issue_leaf(ca, fqdns, max_days)` — emite o leaf assinado pela CA. O **SAN** inclui:
  cada FQDN interceptado, o wildcard `*.dominio` do domínio pai (quando aplicável), e os
  IPs `127.0.0.1` + `::1`. Validade limitada a `max_days` (a casca passa **825**).
- `build_server_config(leaf)` — monta o `ServerConfig` rustls (cadeia + chave) que o motor
  usa para terminar o TLS na `:443`.

No **ligar** (`start_proxy`), todos os FQDNs (primário + `extra_hosts`) entram num único
leaf, antes do bootstrap privilegiado.

---

## 2. Trust store (mkcert)

Gerar a CA não basta — o navegador precisa **confiar** nela (senão, com HSTS no stage, nem
deixa prosseguir). Duas frentes:

- **Sistema + NSS (núcleo):** `tlsca::mkcert_install(caroot)` shell-a `mkcert -install`
  com `CAROOT` apontando para a CA do rcgen — cobre o trust store do SO **e** o NSS
  (Firefox/Chrome). É a parte chata que ninguém quer reimplementar.
- **Navegador, sem root (botão da UI):** o comando `install_cert` roda
  `mkcert -install` com `CAROOT` + **`TRUST_STORES=nss`** — evita o sudo interno do
  mkcert e cobre exatamente o que o navegador valida, **como usuário**. Requer o binário
  `mkcert` no PATH.

O **doctor** (`run_doctor`, check `cert`) reporta confiável só quando a CA existe **e**
está no NSS (`nss_has_mkcert`, best-effort via `certutil`). `[VERIFICAR]` a instalação no
trust store do **sistema** (não-NSS) não é disparada automaticamente pela casca hoje — só
a NSS via `install_cert`. Re-trust só quando a **root** muda.

---

## 3. Privilégios — 1 prompt por sessão

O app roda **sem root**. Eleva pontualmente para: editar o arquivo hosts, bindar `:443`,
e (no Linux) ajustar `setcap`/`sysctl`. Implementação atual: **Linux/pkexec**
(macOS/Windows em `04` §3).

`PrivSession` (`lib.rs`) mantém um `pkexec /bin/sh` **vivo**: a senha é pedida **uma vez**
no spawn (um `run("true")` confirma a autorização; cancelar = erro "elevação não
autorizada"); depois os scripts root seguem por stdin, com um sentinela
`__DSEND__<exit>` para capturar o código de saída. Isso elimina o "duas senhas por
ligar/desligar".

**Segurança do script root** (`build_priv_script`):

- grava o arquivo hosts via **heredoc** (`cat > '/etc/hosts' <<'__DSHOSTS__' …`) — **sem
  temp file**, logo **sem janela de TOCTOU**;
- no ligar, `setcap cap_net_bind_service=+ep <exe>` (libera `:443` p/ o binário) +
  `sysctl -w net.ipv4.ip_unprivileged_port_start=443` (fallback). Ambos com `|| true`;
- `which()` resolve caminhos **absolutos** porque o pkexec zera o `PATH`.

No desligar, `revert_hosts` remove o bloco do devsplit **e** as entradas soltas do FQDN
para loopback — só eleva se houver o que tirar.

Para um bloco **órfão** — quando uma sessão anterior não foi desligada e o bloco
sobrou no `/etc/hosts` (quebrando o acesso real ao stage) — o comando `cleanup_hosts`
(`lib.rs`) reusa essa mesma reversão elevada para removê-lo; é **no-op sem prompt** se já
estiver limpo (pois `revert_hosts` só eleva quando há o que tirar). O `run_doctor` (check
`hosts`) agora é **estado-consciente**: com o proxy **parado** e o bloco **presente** ele
sinaliza o órfão (aviso "clique 'Limpar'"); a UI expõe o botão no painel Saúde (`12` §6).

---

## 4. Segurança — blindar dia 1

Um proxy local em `:443` **com uma CA confiável na máquina** é um vetor sério. As defesas:

### 4.1 Redação de headers (`redact_headers`)

Headers sensíveis nunca chegam à UI/inspector em claro. São **redatados** para
`<redacted>`: `cookie`, `set-cookie`, `x-api-key`, `proxy-authorization`, e **qualquer
header** cujo nome contenha `secret` ou `token`.

> **Exceção deliberada:** `authorization` fica **visível** — é exatamente o que o painel de
> Sessão/JWT (`12` §2) precisa para decodificar o `Bearer`. O `RequestDetail` marca
> `redacted: true` quando algum header foi mascarado.

### 4.2 Validação do passthrough

O catch-all **sempre** valida o cert remoto (`ServerName`=FQDN, roots Mozilla, **sem**
`dangerous()`); o **pin de IP** evita redirecionamento; e há a recusa de loopback +
`X-Devsplit-Hop` → `508` (`01` §4). `verify: false` no YAML é tratado como pé-na-jaca:
nunca em stage real.

### 4.3 Chave da CA

`rootCA-key.pem` é por-usuário, no diretório de config do app — **nunca** versionada nem
logada. (No roadmap, trust 100% Rust remove a dependência do binário mkcert — `00` §8.)

### 4.4 Anti-prod

Regra de **produto**: recusar domínios de produção por padrão; só `stage`/`qa`. É o que
mantém a invariante 4 (`01` §1.1): o serviço local só pode receber o mesmo token quando
está no **mesmo ambiente não-produtivo**.

### 4.5 `/etc/hosts`

Escrever **só** no bloco demarcado, com backup e escrita atômica (`12` §6). É por-máquina:
um bloco sujo quebra o stage só na máquina do dev, e o `single-instance` garante um único
dono de `:443`.

### 4.6 Releases

Assinados/notarizados (`04` §1.2) — planejado.
