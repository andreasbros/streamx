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

        # Source that also carries the prebuilt web UI: rust-embed reads
        # web/dist at compile time. Run `cd web && pnpm build` first.
        srcWithWeb = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || (pkgs.lib.hasSuffix "/web" path && type == "directory")
            || (pkgs.lib.hasInfix "/web/dist" path);
        };

        # Release build of the `streamx` server for one target triple.
        # The server has no C library dependencies (rustls, bundled
        # SQLite; FFmpeg is a runtime process), so cross builds only need
        # a C cross compiler for the few `cc`-built crates. musl targets
        # are linked fully static and verified by the linkage check.
        mkServer = { crossPkgs ? null, target ? null, static ? false }:
          let
            base = commonArgs // {
              src = srcWithWeb;
              pname = "streamx";
              cargoExtraArgs = "-p streamx";
              doCheck = false;
              buildInputs = [ ];
              # Rust's std links -liconv, and the nixpkgs Darwin toolchain
              # resolves it to a store dylib. Retarget it to the SDK copy
              # in /usr/lib (same ABI) so the artifact is self-contained;
              # the stdenv fixup re-signs the binary afterwards.
              postInstall = pkgs.lib.optionalString pkgs.stdenv.isDarwin ''
                for bin in $out/bin/*; do
                  for lib in $(otool -L "$bin" | awk '/\/nix\/store\/.*libiconv/ {print $1}'); do
                    install_name_tool -change "$lib" /usr/lib/libiconv.2.dylib "$bin"
                  done
                done
              '';
            };
            crossEnv = if target == null then { } else
              let
                cc = crossPkgs.stdenv.cc;
                envTarget = builtins.replaceStrings [ "-" ] [ "_" ] target;
                upper = pkgs.lib.toUpper envTarget;
              in
              {
                CARGO_BUILD_TARGET = target;
                depsBuildBuild = [ cc ];
                "CARGO_TARGET_${upper}_LINKER" = "${cc}/bin/${cc.targetPrefix}cc";
                "CC_${envTarget}" = "${cc}/bin/${cc.targetPrefix}cc";
                HOST_CC = "${pkgs.stdenv.cc.nativePrefix}cc";
              } // pkgs.lib.optionalAttrs static {
                CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
              };
            args = base // crossEnv;
          in
          craneLib.buildPackage (args // { cargoArtifacts = craneLib.buildDepsOnly args; });

        linkcheck = craneLib.buildPackage (commonArgs // {
          pname = "streamx-linkcheck";
          cargoExtraArgs = "-p streamx-linkcheck";
          doCheck = false;
          buildInputs = [ ];
        });

        # Assert an artifact's linkage as a flake check.
        linkageCheck = name: drv: binary: policy:
          pkgs.runCommand "linkcheck-${name}" { } ''
            ${linkcheck}/bin/streamx-linkcheck ${drv}/bin/${binary} --policy ${policy}
            touch $out
          '';

      in
      rec {
        # Release outputs per target triple. All require web/dist to be
        # built first (`cd web && pnpm build`).
        #
        #   nix build .#streamx                      native server
        #   nix build .#streamx-x86_64-linux-musl    static server (Linux host)
        #   nix build .#streamx-aarch64-linux-musl   static server (Linux host)
        #   nix build .#streamx-x86_64-darwin        server for the other Mac arch
        #   nix build .#streamx-desktop              Linux desktop (glibc + host graphics)
        #
        # The macOS desktop app is built from the dev shell: GPUI compiles
        # its Metal shaders with Xcode's `metal`, which the Nix sandbox
        # cannot provide. Windows outputs land with the Windows port.
        packages = {
          default = mkServer { };
          streamx = mkServer { };
          streamx-linkcheck = linkcheck;
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          streamx-desktop = craneLib.buildPackage (commonArgs // {
            src = srcWithWeb;
            pname = "streamx-desktop";
            cargoExtraArgs = "-p streamx-desktop";
            doCheck = false;
          });
          streamx-x86_64-linux-musl = mkServer {
            crossPkgs = pkgs.pkgsCross.musl64;
            target = "x86_64-unknown-linux-musl";
            static = true;
          };
          streamx-aarch64-linux-musl = mkServer {
            crossPkgs = pkgs.pkgsCross.aarch64-multiplatform-musl;
            target = "aarch64-unknown-linux-musl";
            static = true;
          };
        } // pkgs.lib.optionalAttrs (system == "aarch64-darwin") {
          streamx-x86_64-darwin = mkServer {
            crossPkgs = pkgs.pkgsCross.x86_64-darwin;
            target = "x86_64-apple-darwin";
          };
        } // pkgs.lib.optionalAttrs (system == "x86_64-darwin") {
          streamx-aarch64-darwin = mkServer {
            crossPkgs = pkgs.pkgsCross.aarch64-darwin;
            target = "aarch64-apple-darwin";
          };
        };

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
          ++ lib.optionals stdenv.isLinux [
            xdotool           # ui-harness: window lookup for screenshots
          ]
          ++ commonBuildInputs
          ++ commonNativeBuildInputs;

          # Environment variables (identical across platforms unless overridden below)
          shellHook = ''
            export RUST_LOG=info
            export RUST_BACKTRACE=1
            # Baked into streamx-desktop at compile time so the app finds
            # mpv even when launched outside this shell (Finder, plain
            # terminal), where the Nix store is not on PATH.
            export STREAMX_MPV_BUILD_PATH="${pkgs.mpv-unwrapped}/bin/mpv"
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

          # Shipped binaries must be self-contained per platform policy.
          linkage-server = linkageCheck "server" packages.streamx "streamx"
            (if pkgs.stdenv.isDarwin then "macos" else "linux-desktop");
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          linkage-server-x86_64-musl =
            linkageCheck "x86_64-musl" packages.streamx-x86_64-linux-musl "streamx" "static";
          linkage-server-aarch64-musl =
            linkageCheck "aarch64-musl" packages.streamx-aarch64-linux-musl "streamx" "static";
          linkage-desktop =
            linkageCheck "desktop" packages.streamx-desktop "streamx-desktop" "linux-desktop";
        };
      }
    );
}
