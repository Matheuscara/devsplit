<!-- Descreva a mudança e o PORQUÊ dela, não só o quê. -->

## O que muda

## Por quê

## Como testei

<!-- ex.: cargo test -p devsplit-core; npm run build; testado no app real (Linux) -->

## Checklist

- [ ] `cargo test -p devsplit-core` passa
- [ ] `cargo fmt --all -- --check` limpo
- [ ] `cargo clippy -p devsplit-core -- -D warnings` limpo
- [ ] `cd app && npm run build` passa
- [ ] Não exponho hostnames/IPs/segredos reais (uso `example.com` / `192.0.2.x`)
- [ ] Atualizei docs/`BLUEPRINT.md` se o comportamento mudou
