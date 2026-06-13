# Política de Segurança

O devsplit é um proxy de desenvolvimento que **termina TLS local** e **edita o
`/etc/hosts`** com elevação de privilégio. Por isso, tratamos relatos de segurança com
prioridade.

## Versões suportadas

O projeto ainda está em fase inicial. Correções de segurança são aplicadas sempre sobre o
branch `main` e a release mais recente.

| Versão | Suportada |
|---|---|
| `main` / último release | ✅ |
| releases anteriores | ❌ |

## Como reportar uma vulnerabilidade

**Não** abra uma issue pública para vulnerabilidades.

Use o canal privado do GitHub:
**Security → Report a vulnerability** (GitHub Security Advisories) no repositório
<https://github.com/Matheuscara/devsplit/security/advisories/new>.

Inclua, se possível:

- descrição do problema e impacto;
- passos para reproduzir (config, comandos, ambiente);
- versão do devsplit e do SO.

Faremos o possível para responder dentro de **7 dias** e coordenar uma correção e
divulgação responsável antes de tornar o relato público.

## Escopo de segurança (invariantes do projeto)

Estes são comportamentos que o devsplit **garante** — uma quebra deles é uma
vulnerabilidade:

- **Passthrough valida o certificado remoto** contra o SNI (FQDN). Nunca usamos
  `insecureSkipVerify` no caminho de passthrough.
- **Redação de segredos no inspector**: headers como `cookie`, `set-cookie`,
  `x-api-key`, `*secret*`, `*token*` e `proxy-authorization` são redatados antes de
  qualquer exibição/exportação. Apenas `authorization` fica visível, de propósito (é o
  que a aba Sessão decodifica).
- **Elevação mínima**: o app roda sem root; a elevação serve apenas para editar o
  `/etc/hosts` e liberar a porta `:443`.
- **Anti-loop por DNS direto**: o IP real do upstream é resolvido ignorando o
  `/etc/hosts`, para o proxy nunca conectar em si mesmo.

Detalhes em [`docs/11-tls-privilegios-seguranca.md`](./docs/11-tls-privilegios-seguranca.md).
