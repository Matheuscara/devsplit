# 12 — Features de runtime

> Deep-dive das funcionalidades que o devsplit oferece enquanto roda — o equivalente ao
> "passo a passo end-to-end". Inspector de tráfego, painel JWT/Sessão, latência,
> detecção de serviços, hot-reload, hosts, re-resolução e notificações. Espelha o motor
> (`proxy/mod.rs`) e a casca (`lib.rs`).

---

## 1. Inspector de tráfego

Cada requisição vira um `TrafficEvent` no motor (`handle`) e segue por um canal mpsc
(`buffer 1024`, `try_send` não-bloqueante) até a casca, que faz três coisas:

1. emite `proxy://traffic` com um **resumo leve** (`TrafficSummary`: id, ts, método, host,
   path, decisão, status, latência, tamanhos) — a lista ao vivo da tela Tráfego;
2. guarda o **evento completo** num **ring buffer de 500** entradas (`AppState.traffic`)
   para o detalhe;
3. emite `proxy://notice` em status `502` (§9).

**Detalhe** (`get_request_detail(id)` → `RequestDetail`): headers de request/response
(pares nome/valor, redatados — `11` §4.1), preview de body e tamanhos, flag `redacted`.
`redact_headers` (`proxy/mod.rs`) substitui por `<redacted>` os sensíveis (`cookie`,
`set-cookie`, `x-api-key`, `proxy-authorization` e qualquer nome contendo `secret`/`token`);
`authorization` fica **visível de propósito** — é o que o painel Sessão/JWT inspeciona (§2).
Coberto pelo teste `redacts_sensitive_headers`.

**Captura de body** (regra do motor): só quando **não é upgrade** e há `Content-Length ≤
256 KiB` (`CAP`); o preview é truncado em **64 KiB** (`PREVIEW_CAP`) com flag
`*_body_truncated`. Streaming/SSE (sem `Content-Length`) e WebSocket (`101`) passam
**direto, sem captura**.

**Export** (`lib/export.ts`, tela Tráfego):

- **copy-as-curl** — `toCurl` monta `curl -X MÉTODO 'https://host/path' -H … --data-raw …`
  (com escape de aspas);
- **export HAR** — `toHar` gera um HAR 1.2 (`creator: devsplit 0.1.0`) com request +
  response + timings (`wait = latency_ms`); `downloadBlob` salva o arquivo;
- **busca/filtro** na lista (tela Tráfego).

---

## 2. Painel JWT / Sessão

A tela **Sessão** decodifica o `Authorization: Bearer` capturado (mantido visível de
propósito — `11` §4.1). `lib/jwt.ts::decodeJwt` faz base64url puro do header + payload
(sem verificar assinatura — é inspeção, não validação) e a UI mostra claims, `exp`, etc.
É o que prova a invariante 4: o serviço **local** recebe o **mesmo token** que o stage.

---

## 3. Latência por requisição

O motor cronometra do início do `handle` até a resposta (`Instant::now()` →
`started.elapsed()`) e preenche `latency_ms` no `TrafficEvent`. Aparece na lista e no HAR.

---

## 4. Detecção de serviços locais

`detect_local_services` (tela Rotas / onboarding) sonda **13 portas comuns** em
`127.0.0.1` com timeout de **120 ms** cada: `3000–3005, 4000, 5000, 5173, 8000,
8080, 8081, 9000`. Retorna as que respondem, com `hint` quando conhecido (`5173` → "Vite
(frontend)", `3000` → "Nest/Node"). Ajuda o dev a montar rotas apontando para o que já
está de pé.

---

## 5. Re-resolução de IP

O IP do stage pode rotacionar (LB de nuvem). Duas vias:

- **Automática:** task que roda a cada **60 s** — faz health-check TCP (timeout 3 s) do IP
  pinado e re-resolve via DNS direto; se o IP mudou **ou** o atual caiu, troca a quente
  (`ArcSwap::store`) e emite `proxy://status`. Abortada no `stop_proxy`.
- **Manual:** `reresolve_upstream` (botão na UI) — re-resolve agora e aplica a quente.

O IP é cacheado em `AppState.resolved_ip` enquanto ligado (evita re-resolver a cada
toggle/reload).

---

## 6. Gestão do `/etc/hosts`

`hostsfile` edita o arquivo de forma idempotente e reversível:

- **bloco demarcado** `# >>> devsplit BEGIN >>>` … `# <<< devsplit END <<<`, **1 nome por
  linha** (limite do Windows);
- **backup** `<hosts>.devsplit.bak` (preserva o original na 1ª vez);
- **escrita atômica** (tempfile no mesmo diretório + rename);
- funções **puras** (`render`, `render_without_block`, `render_revert`) — testadas sem
  disco (idempotência: `render(render(x,e),e) == render(x,e)`);
- **revert** (`render_revert`) remove o bloco **e** linhas soltas que apontem os FQDNs
  para loopback (limpa setups antigos), para o stage voltar a ser alcançável no desligar.

Caminho do arquivo: Windows via `%SystemRoot%\System32\drivers\etc\hosts`; demais
`/etc/hosts`. A escrita real é feita pelo script root (heredoc, `11` §3).

---

## 7. Hot-reload do `devsplit.yaml`

Um watcher no `setup` faz **polling de 2 s** do mtime do `devsplit.yaml`
(`find_config_path`). Ao detectar mudança (salvar o arquivo, `git pull`), recarrega a
config do disco mantendo o perfil ativo (se ainda existir), reaplica no proxy vivo via
`ArcSwap` e emite `proxy://notice` ("devsplit.yaml recarregado"). Conexões em voo não
caem (`01` §8).

---

## 8. Doctor (saúde)

`run_doctor` (`lib.rs`) retorna três checks com `hint` corretivo quando ✗:

| id | label | ok quando | hint se ✗ |
|---|---|---|---|
| `cert` | Certificado confiável | CA existe **e** está no NSS | "CA gerada mas não confiada — clique Instalar certificado" / "CA ainda não gerada — ligue" |
| `hosts` | `/etc/hosts` | **state-aware** (ver abaixo) | "proxy ligado mas /etc/hosts sem a entrada" **ou** "bloco órfão … — clique 'Limpar'" |
| `upstream` | Stage responde | DNS direto resolve | "Sem rota até o stage (VPN ligada?)" |

**`hosts` é consciente do estado** — cruza `running` (proxy ligado?) com `has_block`
(bloco demarcado presente no arquivo?):

| ligado | bloco | resultado |
|---|---|---|
| ✓ | ✓ | **ok** |
| ✓ | ✗ | aviso "proxy ligado mas /etc/hosts sem a entrada" |
| ✗ | ✗ | **ok** |
| ✗ | ✓ | aviso **órfão** "bloco órfão no /etc/hosts (sessão anterior não desligada) — clique 'Limpar'" |

O caso **parado + bloco = órfão** é a novidade: uma sessão anterior fechada sem desligar
deixa o FQDN apontando para loopback e **quebra o acesso real ao stage**. Antes o check
dava "ok" sempre que o bloco existia, mascarando o órfão.

**`cleanup_hosts`** (`lib.rs`, comando Tauri) remove esse bloco órfão sem religar o proxy:
reusa a reversão elevada (`revert_hosts` → `render_revert`), que só pede privilégio quando
há o que tirar — **no-op sem prompt** se já estiver limpo. Na UI é o botão **"Limpar"** que
surge no painel Saúde (tela Rotas) quando o check `hosts` falha (`03` §2); ele chama
`cleanupHosts()` e recheca o doctor. Alternativa por linha de comando em `troubleshooting.md`.

---

## 9. Notificações (toasts)

A casca emite `proxy://notice` (`level`: info/warn/error) que a UI mostra como toast. Os
gatilhos atuais:

- **`502`** numa decisão `Local` → `warn` "Serviço local não respondeu (target) em <path>"
  (§ troubleshooting: o serviço local está fora);
- **`502`** numa decisão `Passthrough` → `error` "Stage inalcançável em <path> (VPN
  ligada?)";
- **hot-reload** → `info` "devsplit.yaml recarregado".

> Não confundir **`502`** (upstream falhou — serviço local fora ou stage inalcançável) com
> **`508 Loop Detected`** (a request reentrou no proxy — anti-loop, `01` §4). Diagnóstico
> em `troubleshooting.md`.
