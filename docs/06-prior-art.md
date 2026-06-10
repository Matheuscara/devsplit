# 06 — Prior art e diferenciação

> Onde está o gap de mercado e por que o devsplit não é "reinventar a roda". O equivalente
> ao doc de go-to-market: posicionamento e concorrência. Síntese de `BLUEPRINT.md` §3.

---

## 1. O gap

A combinação exata —

> **reverse-proxy transparente por domínio + split por path-prefix com passthrough para o
> remoto real (cert validado) + TLS+DNS local auto-geridos + app de gerência por time,
> fora de k8s**

— **não é atendida por nenhuma ferramenta única**, e ninguém com **interface visual**.
Quem chega perto (Charles/Proxyman) é forward-proxy de **debug individual**, não
infraestrutura de dev transparente por time. A issue
[Localias#39](https://github.com/peterldowns/localias/issues/39) pede exatamente "proxy
de paths diferentes do mesmo domínio para portas diferentes" — aberta, ninguém entregou.

---

## 2. Tabela comparativa

Legenda: ✅ sim · ⚠️ parcial · ❌ não.

| Classe | Exemplos | Transparente (domínio) | Split path-prefix p/ remoto | TLS+DNS local auto-geridos | Fora de k8s | UI / UX de time |
|---|---|---|---|---|---|---|
| **k8s inner-loop** | Telepresence, mirrord, Tilt, DevSpace | ⚠️ | ⚠️ (por header/serviço) | n/a | ❌ exige cluster | ⚠️ |
| **Tunnels** | ngrok, localtunnel, cloudflared | ✅ (público) | ⚠️ (problema inverso) | ✅ (edge) | ✅ | ⚠️ |
| **Debug/MITM** | Charles, Proxyman, Whistle, mitmproxy | ❌ (forward proxy) | ✅ (map remote/local) | ⚠️ (CA manual) | ✅ | ✅ (debug individual) |
| **Reverse proxies** | Caddy, Traefik, nginx | ✅ | ✅ (nativo) | ⚠️ | ✅ | ❌ (engine cru) |
| **Dev-env DNS+TLS** | Valet, DDEV, Lando, Localias | ✅ | ❌ (domínio→porta local) | ✅ | ✅ | ⚠️ (CLI) |
| **devsplit** | — | ✅ | ✅ | ✅ | ✅ | ✅ (app desktop) |

---

## 3. Posicionamento

- **Substitui** o setup hand-rolled Traefik + mkcert + `/etc/hosts` + bash da implementação
  de referência — mesma ideia, **produtizada**: app cross-platform, UI, config de time.
- **Não é** um proxy de debug de sistema: é reverse-proxy **por domínio**, transparente, o
  front não muda.
- **Não é** k8s inner-loop: roda fora de cluster, na máquina do dev.
- O valor é a **costura** (TLS confiável + validação de cert remoto por SNI + edição de
  hosts + DNS direto anti-loop) **+ a interface**. As partes difíceis são reusáveis
  (mkcert, rustls, rcgen, hickory); ninguém as costurou num app com UX de time.

---

## 4. Nome

Working name **devsplit** — descritivo e claro. Alternativas brandáveis no
`BLUEPRINT.md` §13 (Detour, Splice, Sidedoor, Patchbay). Checar disponibilidade no GitHub
e nos gerenciadores (Homebrew/Scoop) antes de cravar. `[VERIFICAR]` disponibilidade.

---

## 5. Risco honesto

O maior risco **não** é o motor (provado por testes — `STATUS.md`), e sim a **casca de
plataforma**: WebKitGTK instável no Linux (`05` §3) e as armadilhas de TLS/DNS/elevação
cross-platform (`04` §3, `11`). Tudo factível; o diferencial de produto é claro.
