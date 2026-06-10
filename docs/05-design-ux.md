# 05 — Design e UX

> A linguagem visual, os design tokens reais (`app/src/index.css`), as telas, as regras de
> animação (e a pegadinha do WebKitGTK no Linux) e acessibilidade. Referências de
> dev-tool premium: Linear, Raycast, Tailscale, OrbStack.

---

## 1. Design tokens (implementados — `@theme` em `index.css`)

Paleta **dark-only** (cinzas frios + **um único accent** verde, que é o elemento "vivo" —
o botão on/off). Tailwind v4 via `@theme`:

| Token | Valor | Uso |
|---|---|---|
| `--color-bg` | `#0b0c0e` | Fundo. |
| `--color-surface` / `--color-surface-2` | `#15171a` / `#1b1e22` | Cartões/painéis. |
| `--color-border` | `#23262b` | Bordas (default em `*`). |
| `--color-text` / `--color-muted` | `#e6e7e9` / `#9aa0a6` | Texto / secundário. |
| `--color-accent` / `--color-accent-dim` | `#34d399` / `#2bb588` | **O accent** (verde = ativo). |
| `--color-warn` / `--color-danger` | `#f0b429` / `#f05252` | Avisos / erros (toasts, doctor). |
| `--radius-md` | `8px` | Raio padrão. |
| `--font-sans` | **Inter** (com `cv02/03/04/11`) | Tipografia; `color-scheme: dark`. |

Princípios concretos: **um único accent colorido**, densidade de dados para power-user,
escala consistente, scrollbars discretas estilizadas.

---

## 2. Telas

Layout: barra lateral de navegação + cabeçalho com o **botão grande ligar/desligar** +
`StatusDot` (verde = ativo) + host de stage interceptado. As seis telas (`03` §2): Rotas,
Tráfego, Sessão, Certificado, Hosts, Config. O **doctor visual** (✅ cert · ✅ hosts · ✅
upstream) com botão "corrigir" quando ✗.

Componentes de experiência: **onboarding** animado na 1ª execução, **command palette**
(Cmd-K, estilo Raycast), **toasts** (consomem `proxy://notice`), **SystemFlow** (diagrama
animado do fluxo). Ver `03` §3.

---

## 3. Animação — regras e a pegadinha do WebKitGTK

Animação **só no ocasional** (abrir painel, onboarding, toast de "cert instalado") —
**nunca** no toggle repetido. As animações existentes são CSS puro (`@keyframes
devsplit-overlay-in` / `-card-in` / `-step-in`), animando **só `opacity` e `transform`**.

**Por quê:** no Linux o webview é o **WebKitGTK**, que borra o app durante animação CSS e
está atrás do Chromium em features modernas (`backdrop-filter`, `contenteditable`). Regra
firme: CSS conservador, animar só `transform`/`opacity`, evitar `backdrop-filter`/
`contenteditable`, **distribuir no Linux via AppImage** (embute o WebKitGTK 4.1,
previsível) e **QA real nos 3 SOs**. É o maior risco de UI do projeto
(`BLUEPRINT.md` §7.7).

---

## 4. Dark mode e acessibilidade

- **Dark mode:** a paleta é fixada dark (`color-scheme: dark`; `html` carrega `dark`). Um
  tema claro é roadmap (`00` §8, v0.2).
- **Acessibilidade:** o accent verde sobre fundo escuro precisa de contraste verificado;
  estados de foco e navegação por teclado (a command palette já é teclado-first).
  `[VERIFICAR]` razões de contraste com ferramenta no CI antes de marketing.

---

## 5. Referências visuais

`design/mockup.html` (mockup navegável), `design/icon.svg` / `icon.png` (ícone). A
linguagem-alvo é "Linear/Vercel": calma, densa, um acento, sem cromo desnecessário.
