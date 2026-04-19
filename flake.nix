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
          # Do NOT pin apple-sdk_15 here: nixpkgs stdenv already ships
          # its own pinned apple-sdk and the cc wrapper will complain
          # about "conflicting DEVELOPER_DIR" if we add another one.
          # Legacy stubs like darwin.libobjc and
          # darwin.apple_sdk.frameworks.* were removed in nixpkgs — see
          # https://nixos.org/manual/nixpkgs/stable/#sec-darwin-legacy-frameworks
          libiconv
        ] ++ lib.optionals stdenv.isLinux [
          # GPUI / graphics
          vulkan-loader
          vulkan-headers
          shaderc
          mesa                      # Mesa provides Vulkan ICDs (Intel/AMD/LVP)
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

            # Testing / graphics / diagnostics
            playwright-driver.browsers
            imagemagick
            vulkan-tools      # vulkaninfo, vkcube
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
          '' + pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
            # Prefer Xcode toolchain over nix-bundled apple-sdk when Xcode
            # is installed. GPUI's Metal shader compiler ("metal") lives
            # only in Xcode, so DEVELOPER_DIR must point at it. Safe
            # fallback: keep nix values if Xcode is absent.
            if [ -d /Applications/Xcode.app ]; then
              export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
              if sdkpath=$(xcrun --sdk macosx --show-sdk-path 2>/dev/null); then
                export SDKROOT="$sdkpath"
              fi
            fi
            # nixpkgs bundles a 2019-era xcbuild `xcrun` that cannot
            # dispatch the modern Metal Toolchain (which moved to a
            # cryptex mount in macOS 15). GPUI's build.rs invokes
            # `xcrun metal`, so we need Apple's real xcrun first on
            # PATH. /usr/bin is always Apple's stock binaries on macOS.
            if [ -x /usr/bin/xcrun ]; then
              export PATH="/usr/bin:$PATH"
            fi
            # Belt and braces: also put the Metal Toolchain itself
            # directly on PATH so a bare `metal` invocation works, and
            # any future xcrun lookup that happens to point there
            # resolves correctly.
            for d in /var/run/com.apple.security.cryptexd/mnt/com.apple.MobileAsset.MetalToolchain*/Metal.xctoolchain/usr/bin; do
              if [ -d "$d" ]; then
                export PATH="$d:$PATH"
                break
              fi
            done
          '' + pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            # Runtime library path for GPUI + libmpv on Linux. Without this,
            # Wayland/X11/Vulkan dlopen fails at app launch with errors like
            # `NoWaylandLib` or `libvulkan.so.1: cannot open shared object`.
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath [
              pkgs.wayland
              pkgs.libxkbcommon
              pkgs.vulkan-loader
              pkgs.mesa
              pkgs.libxcb
              pkgs.libx11
              pkgs.libxcursor
              pkgs.libxi
              pkgs.libxrandr
              pkgs.dbus
              pkgs.libGL
              pkgs.fontconfig
              pkgs.freetype
              pkgs.mpv-unwrapped
              pkgs.ffmpeg-full
            ]}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

            # Vulkan ICD discovery. We ship mesa from nix (buildInputs above)
            # which provides ICDs + matching .so files in the nix store.
            # Users can override with STREAMX_VK_ICD_OVERRIDE, e.g. to point
            # at NVIDIA drivers on the host.
            if [ -n "''${STREAMX_VK_ICD_OVERRIDE:-}" ]; then
              export VK_DRIVER_FILES="$STREAMX_VK_ICD_OVERRIDE"
              export VK_ICD_FILENAMES="$STREAMX_VK_ICD_OVERRIDE"
            else
              __sxicd=""
              for d in \
                "${pkgs.mesa}/share/vulkan/icd.d" \
                /run/opengl-driver/share/vulkan/icd.d; do
                if [ -d "$d" ]; then
                  for f in "$d"/*.json; do
                    [ -f "$f" ] || continue
                    if [ -z "$__sxicd" ]; then __sxicd="$f"; else __sxicd="$__sxicd:$f"; fi
                  done
                fi
              done
              if [ -n "$__sxicd" ]; then
                export VK_DRIVER_FILES="$__sxicd"
                export VK_ICD_FILENAMES="$__sxicd"
              fi
              unset __sxicd
            fi

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
