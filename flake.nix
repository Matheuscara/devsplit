{
  # devsplit no NixOS / Nix.
  #
  # Embrulha o .AppImage publicado no GitHub Release (appimageTools.wrapType2):
  # roda num FHS-env com webkit etc. resolvidos e com mkcert + nssTools INJETADOS
  # no PATH do app (via extraPkgs) — entao a dep do mkcert fica resolvida no Nix
  # sem o usuario instalar nada a mais (mkcert chama o certutil, do nssTools).
  #
  # So x86_64-linux: o release Linux e o AppImage amd64 (nao ha build arm64 Linux).
  #
  # Instalar (sem clonar):
  #   nix profile install github:Matheuscara/devsplit
  # Rodar uma vez:
  #   nix run github:Matheuscara/devsplit
  #
  # ATUALIZAR A CADA RELEASE:
  #   1) suba `version` p/ a tag nova;
  #   2) troque `sha256` por `pkgs.lib.fakeHash` e rode `nix build` -> o Nix imprime
  #      o hash real ("got: sha256-...") — cole de volta em sha256;
  #   3) `nix flake update` p/ refrescar o nixpkgs no flake.lock (opcional).
  #
  # NixOS — runtime (no configuration.nix do usuario, NAO no flake):
  #   - security.polkit.enable = true;  (pkexec p/ editar /etc/hosts)
  #   - o `setcap` do app FALHA no /nix/store (read-only); o devsplit cai no
  #     fallback sysctl p/ bindar a :443. Garanta:
  #       boot.kernel.sysctl."net.ipv4.ip_unprivileged_port_start" = 443;
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      version = "0.1.0";

      appimage = pkgs.fetchurl {
        url = "https://github.com/Matheuscara/devsplit/releases/download/v${version}/devsplit_${version}_amd64.AppImage";
        # hash do AppImage publicado (v0.1.0). No bump: ver "ATUALIZAR A CADA RELEASE" acima.
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
      packages.${system} = {
        default = devsplit;
        devsplit = devsplit;
      };
      apps.${system}.default = {
        type = "app";
        program = "${devsplit}/bin/devsplit";
      };
    };
}
