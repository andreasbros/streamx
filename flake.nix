{
  description = "StreamX - Torrent Video Streaming Player";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };

  # For faster CI/dev builds, consider using a binary cache:
  #   - Cachix: `cachix use <your-cache>` after `cachix create <your-cache>`
  #     then add `cachix push <your-cache>` to your CI pipeline
  #   - Self-hosted nix binary cache: configure `nix.settings.substituters`
  #     and `nix.settings.trusted-public-keys` in your NixOS/nix config
  #   - GitHub Actions cache: use DeterminateSystems/magic-nix-cache-action

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
          targets = [ "x86_64-unknown-linux-musl" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          buildInputs = with pkgs; [
            rustToolchain
            openssl
            openssl.dev
            pnpm
            nodejs_22
            imagemagick
            sqlite
          ];

          shellHook = ''
            export RUST_LOG=info
            export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
          '';
        };
      }
    );
}
