# Tasks

Tarefas pendentes do devsplit. Pegar quando der.

## Em aberto

_(nada no momento)_

## Concluído

### Tráfego não está funcionando
- **Reportado:** 2026-06-10 · **Resolvido:** 2026-06-10 (voltou a funcionar no retest).
- Investigação não achou bug no código: mock (`npm run dev`) funciona ponta a ponta
  (lista enche, clique abre o detalhe, headers/body/cURL/HAR ok), `cargo test -p devsplit-core`
  passa (18), `tsc -b` limpa, e o caminho real está coerente (`serve` → `handle` emite
  `TrafficEvent`; casca emite `proxy://traffic` em camelCase batendo com `TrafficEntry`).
- Sem alteração de código. Se voltar a falhar, capturar o sintoma exato (lista vazia vs.
  travamento vs. erro) e se é no app Tauri real ou no dev.
