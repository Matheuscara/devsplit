# Troubleshooting

> Sintomas comuns e o que cada um significa. A maioria cai em "serviço local fora",
> "stage inalcançável" ou "cert não confiável". Conceitos em `01`; features em `12`.

---

## 502 numa rota — "Serviço local não respondeu"

O toast `warn` "Serviço local não respondeu (127.0.0.1:porta) em <path>" significa que a
rota **casou local**, mas nada respondeu naquela porta.

- O serviço local **não está de pé** ou está em **outra porta** → suba-o, ou ajuste o
  `target` da rota (tela Rotas / `devsplit.yaml`).
- Use a **detecção de serviços** (`12` §4) para ver o que está escutando.

## 502 no catch-all — "Stage inalcançável (VPN ligada?)"

O toast `error` "Stage inalcançável em <path>" significa que o **passthrough** falhou ao
conectar no IP real.

- O stage só é acessível **via VPN** e a VPN está desligada → ligue a VPN. O doctor
  (`upstream`) também reporta "Sem rota até o stage (VPN ligada?)".
- O IP do stage **rotacionou** → clique **re-resolver** (ou espere o ciclo de 60 s, `12`
  §5).

## 508 Loop Detected

A resposta `508` (com corpo "devsplit: loop detectado") significa que a request **reentrou
no proxy** (chegou com `X-Devsplit-Hop`). É o anti-loop (`01` §4). Causas:

- algo está reenviando tráfego do devsplit para ele mesmo;
- o IP do passthrough resolveu para **loopback** (o passthrough também aborta nesse caso).
  Verifique se o `passthrough.resolve` é o FQDN **real** e se o DNS direto funciona
  (`cargo test -p devsplit-core -- --ignored`).

## Certificado não confiável no navegador

Cadeado vermelho / `NET::ERR_CERT_AUTHORITY_INVALID`, ou (com **HSTS**) o navegador nem
deixa prosseguir:

- a CA local ainda não está no trust store do navegador → **Certificado → Instalar
  certificado** (`install_cert`, NSS, `11` §2);
- confira que o binário `mkcert` está no PATH (`mkcert -version`);
- Firefox/Chrome usam **NSS** — o doctor (`cert`) só fica ✅ quando a CA está no NSS.

## Conflito na porta 443 (Traefik/Docker antigo)

"falha ao bindar 0.0.0.0:443" = **algo já tem a `:443`**. Tipicamente o setup antigo
(Traefik/`local-proxy` em Docker) que o devsplit veio substituir.

- pare o container/serviço que ocupa a `:443` (`docker compose down`, etc.);
- o `single-instance` garante que **outro devsplit** não seja o culpado.

## `pkexec` cancelado / sem senha

"elevação não autorizada (pkexec cancelado?)" = o prompt de senha foi cancelado ou falhou.
Religue e autorize. A senha é pedida **uma vez por sessão**; se a sessão root caiu, o
próximo comando privilegiado reabre o prompt.

## OOM / lentidão ao compilar a casca Tauri

Compilar `app/src-tauri` puxa rustls/hyper/hickory + Tauri — pesado em RAM. Se o build for
morto por OOM:

- compile com **menos paralelismo**: `cargo build -j 2` (ou menor);
- feche outros processos pesados; considere mais swap;
- para iterar **sem** o webview, trabalhe no **núcleo** (`cargo test -p devsplit-core`) e
  no **frontend no navegador** (`npm run dev`) — ambos leves.

## App Tauri não compila — `webkit2gtk` ausente

No Linux, sem `webkit2gtk-4.1`/`libsoup3` a casca não compila (`getting-started.md` §1.1).
O núcleo e o frontend-no-navegador rodam mesmo assim.

## `devsplit.yaml` não foi encontrado

O app procura subindo do cwd até a raiz do repo, depois no diretório de config (`02`
§3.6). Garanta que o arquivo está na raiz do repo (copiado de `examples/devsplit.yaml`) ou
abra o app a partir de lá.

## O `/etc/hosts` "travou" apontando para local

Se o app fechou sem reverter (crash), pode sobrar o bloco `# >>> devsplit BEGIN >>>`. O
revert no desligar limpa o bloco **e** linhas soltas do FQDN para loopback (`12` §6); em
último caso, remova o bloco demarcado à mão — o original está no backup
`<hosts>.devsplit.bak`.

## Janela branca / "Could not connect to localhost"

A janela abre **em branco** (ou o log diz `Could not connect to localhost`) porque a
webview tentou carregar o vite dev server em vez dos assets. Acontecia quando havia
`devUrl` na config e o vite não estava de pé.

- O `devUrl` foi **removido** do `tauri.conf.json` → rode via assets **embutidos** com
  `cd app/src-tauri && cargo run` (`getting-started.md` §2.3). Não é preciso subir o vite.
- Se mexeu no frontend, rebuilde antes: `cd app && npm run build` (`desenvolvimento.md`
  §2.1).

## `npm run build` / `vite build` morto (exit 137 / OOM)

O build do frontend morre com **exit 137** (OOM-killer). É **pressão de memória do
ambiente**, não bug no código — o `tsc -b` passando confirma que o código está ok.

- Libere RAM (feche apps, esvazie o swap) e rode `npm run build` de novo;
- só o **frontend** precisa desse build — o núcleo (`cargo test -p devsplit-core`) é leve.

## Stage inacessível com o proxy desligado / bloco órfão no `/etc/hosts`

Você desligou o split mas o **stage continua caindo em local** (ou saiu do app sem
desligar). Sobrou um **bloco órfão** no `/etc/hosts` (FQDN → 127.0.0.1). O **doctor**
(`hosts`) sinaliza isso como aviso quando o proxy está parado mas o bloco existe.

- No painel **Saúde** (tela Rotas), clique **Limpar** — chama `cleanup_hosts`, que remove
  o bloco via a reversão elevada (no-op se já limpo);
- à mão: `sudo sed -i '/# >>> devsplit BEGIN >>>/,/# <<< devsplit END <<</d' /etc/hosts`.
