# 03 — Frontend (UI React)

> A camada que o usuário vê (L1). Stack **real** implementada, as seis telas, os
> componentes, a camada `lib/` (IPC, JWT, export) e o **IPC mock** que torna a UI
> explorável no navegador sem Tauri. Consome o contrato IPC de `02` §3.

---

## 1. Stack (implementada)

| Papel | Escolha | Nota |
|---|---|---|
| Framework | **React 19** | `app/src/main.tsx` → `App.tsx`. |
| Bundler | **Vite 6** | `npm run dev` (`:5173`), `npm run build` (`tsc -b && vite build`). |
| Estilo | **Tailwind CSS v4** (`@tailwindcss/vite`) | Tokens via `@theme` em `index.css` (`05` §1). |
| Ícones | **lucide-react** | Único pacote de UI de terceiros. |
| Componentes | **próprios** (`components/`) | `Button`, `Card`, `Switch`, `Badge`, `StatusDot`, etc. |
| Ponte nativa | **`@tauri-apps/api`** | `invoke`/`listen` em `lib/ipc.ts`. |

> **Divergência vs. `BLUEPRINT.md` §5/§6:** o desenho citava **shadcn/ui + Motion +
> TradingView Lightweight Charts**. A implementação atual **não** os embute — usa
> componentes hand-rolled, animações em CSS puro (`@keyframes` em `index.css`) e ainda
> **não** tem o gráfico de req/s em canvas. São itens de roadmap (`00` §8), não entregues.

---

## 2. Telas (views)

A navegação é uma barra lateral (`App.tsx`, `NAV`), com um cabeçalho que tem o **botão
grande ligar/desligar** + status + host interceptado. As seis telas (`ViewId`):

| Tela | Arquivo | O que mostra/faz |
|---|---|---|
| **Rotas** | `views/RoutesView.tsx` | Lista de rotas locais (`/x → 127.0.0.1:porta`) com toggle por rota, add/remove; badge `local`/`passthrough`; seletor de perfil. |
| **Tráfego** | `views/TrafficView.tsx` | Lista ao vivo das requisições (método/host/path/decisão/status/latência); ao clicar, detalhe com headers/body, **copy-as-curl**, **export HAR**, busca/filtro. |
| **Sessão** | `views/SessionView.tsx` | Painel JWT: decodifica o `Authorization: Bearer` capturado (header/payload, `exp`, claims). |
| **Certificado** | `views/CertificateView.tsx` | Estado do cert/CA + botão "Instalar certificado" (`installCert`). |
| **Hosts** | `views/HostsView.tsx` | Estado do bloco no `/etc/hosts`. |
| **Config** | `views/ConfigView.tsx` | Visão do `devsplit.yaml` ativo / ambiente. |

O **doctor** (saúde: cert/hosts/upstream) aparece no painel **Saúde** da tela Rotas
(`runDoctor`/`onRerunDoctor`); quando o check `hosts` falha (orfão — `12` §8) surge um
botão **"Limpar"** que chama `cleanupHosts()` e recheca o doctor.

---

## 3. Componentes notáveis

| Componente | Papel |
|---|---|
| `components/Onboarding.tsx` | Onboarding animado na primeira execução (flag `devsplit:onboarded` no `localStorage`); animações `@keyframes devsplit-*`. |
| `components/CommandPalette.tsx` | Command palette estilo Raycast (Cmd-K): fuzzy match por subsequência com bônus de contiguidade; comandos para navegar, ligar/desligar, trocar perfil, atualizar. |
| `components/Toast.tsx` | `Toaster` — consome `proxy://notice` (info/warn/error). |
| `components/SystemFlow.tsx` | Diagrama animado do fluxo front→devsplit→local/passthrough. |
| `components/StatusDot.tsx` | Indicador de status (verde = ativo). |
| `Button`/`Card`/`Switch`/`Badge` | Primitivos de UI próprios. |

---

## 4. Camada `lib/`

| Arquivo | Conteúdo |
|---|---|
| `lib/ipc.ts` | **Fonte da verdade dos tipos da UI** (`Status`, `Route`, `DoctorCheck`, `Profiles`, `TrafficEntry`, `RequestDetail`, `LocalService`, `Notice`) + a interface `DevsplitIpc` (inclui `cleanupHosts()` → `invoke("cleanup_hosts")`, usado pelo botão "Limpar" do painel Saúde). Dois backends: real (`invoke`/`listen`) e mock. |
| `lib/jwt.ts` | `decodeJwt`/`encodeJwt` (base64url puro). `decode` alimenta a tela Sessão; `encode` cunha tokens decodificáveis no mock. A assinatura **nunca** é verificada no cliente. |
| `lib/export.ts` | `toCurl` (monta `curl -X … -H … --data-raw …`, com escape de aspas) e `toHar` (HAR 1.2, `creator: devsplit 0.1.0`) + `downloadBlob`. |
| `lib/cn.ts` | Helper de className. |

### 4.1 Seleção de backend (real vs mock)

```ts
const isTauri = "__TAURI_INTERNALS__" in window;
export const ipc = isTauri ? createTauriIpc() : createMockIpc();
export const RUNTIME = isTauri ? "tauri" : "mock";
```

- **Tauri (`createTauriIpc`):** cada método é um `invoke` do comando correspondente;
  `onTraffic`/`onStatus`/`onNotice` são `listen` dos eventos `proxy://*`.
- **Mock (`createMockIpc`):** auto-contido, para `npm run dev`/`preview` e CI build —
  gera status, rotas, tráfego sintético (com bodies plausíveis e tokens JWT cunhados via
  `encodeJwt`) e responde a todos os comandos. **Toda a UI é explorável headless**, sem
  webview nem privilégio.

---

## 5. Como rodar

```bash
cd app
npm install
npm run dev      # http://localhost:5173 — RUNTIME = "mock"
npm run build    # tsc -b && vite build (gate de tipos + bundle)
```

Dentro do app Tauri (`cargo tauri dev`, na raiz/`app`) o mesmo bundle roda com
`RUNTIME === "tauri"` e fala com o núcleo de verdade. Pré-requisitos nativos em
`getting-started.md` §1.
