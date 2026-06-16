{
  # devsplit no NixOS / Nix.
  #
  # Embrulha o .AppImage publicado no GitHub Release (appimageTools.wrapType2):
  # roda num FHS-env com webkit etc. resolvidos e com mkcert + nssTools INJETADOS
  # no PATH do app (via extraPkgs) — entao a dep do mkcert fica resolvida no Nix
  # sem o usuario instalar nada a mais (mkcert chama o certutil, do nssTools).
  #
  # Instalar (sem clonar):
  #   nix profile install github:Matheuscara/devsplit
  # Rodar uma vez:
  #   nix run github:Matheuscara/devsplit
  #
  # ATUALIZAR A CADA RELEASE:
  #   1) suba `version` p/ a tag nova;
  #   2) deixe `sha256 = lib.fakeHash;` (linha abaixo);
  #   3) `nix build` -> o Nix imprime o hash real ("got: sha256-...") — cole em sha256.
  #
  # NixOS — runtime (no configuration.nix do usuario, NAO no flake):
  #   - security.polkit.enable = true;  (pkexec p/ editar /etc/hosts)
  #   - o `setcap` do app FALHA no /nix/store (read-only); o devsplit cai no
  #     fallback sysctl p/ bindar a :443. Garanta:
  #       boot.kernel.sysctl."net.ipv4.ip_unprivileged_port_start" = 443;
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = pkgs.lib;

        version = "0.1.0";

        appimage = pkgs.fetchurl {
          url = "https://github.com/Matheuscara/devsplit/releases/download/v${version}/devsplit_${version}_amd64.AppImage";
          # hash do AppImage publicado (v0.1.0). No bump de versao: troque por lib.fakeHash, rode `nix build` e cole o "got: sha256-...".
          sha256 = "sha256-TXvxGslntmbJQMq7d4Xht/tBY4sIuJIS1vQKK2Tqdfo=";
        };

        devsplit = pkgs.appimageTools.wrapType2 {
          pname = "devsplit";
          inherit version;
          src = appimage;
          # disponiveis no PATH do app em runtime:
          extraPkgs = p: [ p.mkcert p.nssTools ];
        };
      in {
        packages = {
          default = devsplit;
          devsplit = devsplit;
        };
        apps.default = {
          type = "app";
          program = "${devsplit}/bin/devsplit";
        };
      });
}
