{
  description = "StreamX - torrent-based streaming player (server + desktop)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Nightly Rust toolchain pinned via rust-toolchain.toml.
        # Nightly is required for edition 2024 (gpui, future desktop crate).
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Shared dependencies for building any crate in the workspace.
        commonBuildInputs = with pkgs; [
          openssl
          sqlite
          ffmpeg-full
          mpv-unwrapped              # libmpv for future desktop video playback
        ] ++ lib.optionals stdenv.isDarwin [
          apple-sdk_15
          libiconv
          darwin.libobjc
        ] ++ lib.optionals stdenv.isLinux [
          # GPUI / graphics
          vulkan-loader
          vulkan-headers
          shaderc
          libxkbcommon
          wayland
          libx11
          libxcb
          libxcursor
          libxi
          libxrandr
          # MPRIS media session on Linux
          dbus
          dbus.dev
          # Hardware video decode (existing)
          libva
          libdrm
          intel-media-driver
        ];

        commonNativeBuildInputs = with pkgs; [
          pkg-config
          cmake
        ];

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
          buildInputs = commonBuildInputs;
          nativeBuildInputs = commonNativeBuildInputs;
        };

      in
      {
        # Packages intentionally deferred. `nix build .#default` would need
        # the frontend pre-built at web/dist/ because rust-embed resolves
        # that path at compile time. Proper packaging lands in Phase 8.
        # Until then, build inside the dev shell:
        #   cd web && pnpm install && pnpm build && cd ..
        #   cargo build --release --manifest-path crates/server/Cargo.toml

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustToolchain
            rust-analyzer

            # Cargo ecosystem
            cargo-watch
            cargo-edit
            cargo-nextest

            # Frontend toolchain
            pnpm
            nodejs_22

            # Testing / graphics
            playwright-driver.browsers
            imagemagick
          ]
          ++ commonBuildInputs
          ++ commonNativeBuildInputs;

          # Environment variables (identical across platforms unless overridden below)
          shellHook = ''
            export RUST_LOG=info
            export RUST_BACKTRACE=1
            export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
            export PLAYWRIGHT_BROWSERS_PATH="${pkgs.playwright-driver.browsers}"
            export PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=true
          '' + pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            export LIBVA_DRIVERS_PATH="${pkgs.intel-media-driver}/lib/dri:${pkgs.libva}/lib/dri''${LIBVA_DRIVERS_PATH:+:$LIBVA_DRIVERS_PATH}"
            export LIBVA_DRIVER_NAME=iHD
          '' + ''
            echo "StreamX dev shell ready"
            echo "  Rust:     $(rustc --version)"
            echo "  Node:     $(node --version)"
            echo "  Platform: ${system}"
          '';
        };

        # cargo clippy + fmt as reusable checks. Run with: nix flake check
        checks = {
          clippy = craneLib.cargoClippy (commonArgs // {
            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });

          fmt = craneLib.cargoFmt {
            src = ./.;
          };
        };
      }
    );
}
