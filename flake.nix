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

        # Web UI built hermetically from the git-tracked sources. A
        # locally built web/dist can never reach the sandbox: the flake's
        # source copy is git-filtered and dist/ is gitignored.
        webDist = pkgs.stdenv.mkDerivation (finalAttrs: {
          pname = "streamx-web";
          version = "0.1.0";
          src = ./web;
          nativeBuildInputs = [ pkgs.nodejs_22 pkgs.pnpm pkgs.pnpmConfigHook ];
          pnpmDeps = pkgs.fetchPnpmDeps {
            inherit (finalAttrs) pname version src;
            fetcherVersion = 2;
            hash = "sha256-D5CgLwtqdRv8gjm2PahzyVN3VypEhCQgHoVUqFZXDHQ=";
          };
          buildPhase = ''
            runHook preBuild
            pnpm build
            runHook postBuild
          '';
          installPhase = ''
            runHook preInstall
            cp -r dist $out
            runHook postInstall
          '';
        });

        # Cargo sources plus the binary assets include_bytes!/rust-embed
        # need: desktop icons and the web UI sources referenced by the
        # desktop crate (filterCargoSources keeps only Cargo/Rust files).
        srcBase = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || (type == "regular"
              && (pkgs.lib.hasInfix "/crates/desktop/assets/" path
                || pkgs.lib.hasInfix "/web/src/assets/" path));
        };

        # rust-embed reads web/dist at server compile time; graft the
        # Nix-built dist onto the cargo source tree.
        srcWithWeb = pkgs.runCommand "streamx-src-with-web" { } ''
          cp -r ${srcBase} $out
          chmod -R u+w $out
          mkdir -p $out/web
          cp -r ${webDist} $out/web/dist
        '';

        # pkgsStatic with fixes for packages whose static build is broken
        # in this nixpkgs pin.
        staticSetOf = ps: ps.pkgsStatic.extend (final: prev: {
          # The Vulkan loader dlopens ICDs by design and cannot be built
          # as a static library. Use the musl-dynamic build: libplacebo
          # and mpv only need it at link time, and the final desktop
          # binary links the host's libvulkan.so.1, which the
          # linux-desktop linkage policy allowlists. Wayland docs are
          # off to keep doxygen/spdlog (broken tests on musl) out of
          # the closure.
          vulkan-loader = ps.pkgsMusl.vulkan-loader.override {
            wayland = ps.pkgsMusl.wayland.override { withDocumentation = false; };
          };

          # Same reasoning for ALSA: a static alsa-lib cannot dlopen the
          # host's PipeWire/Pulse routing plugins, leaving the app
          # silent on modern desktops. Link the host's libasound.so.2
          # dynamically (allowlisted) instead.
          alsa-lib = ps.pkgsMusl.alsa-lib;

          # shaderc unconditionally also builds shaderc_shared, which the
          # static musl toolchain cannot link. Drop the shared target;
          # the static archives then stay in the "lib" output (fixup
          # deletes empty outputs, so the upstream move of *.a into the
          # separate "static" output would leave "lib" empty and fail
          # the build).
          shaderc = prev.shaderc.overrideAttrs (o: {
            outputs = pkgs.lib.remove "static" o.outputs;
            postInstall = "";
            # shaderc.pc advertises the dropped shared library; point
            # consumers (libplacebo, mpv) at the static archives
            # instead. nixpkgs unvendors glslang/SPIRV-Tools, so
            # "combined" is not self-contained and the link closure has
            # to be spelled out. postFixup: the .pc lands in $dev only
            # during the multi-output fixup.
            postFixup = (o.postFixup or "") + ''
              substituteInPlace "$dev/lib/pkgconfig/shaderc.pc" \
                --replace-fail -lshaderc_shared \
                  "-lshaderc_combined -L${final.glslang}/lib -lglslang -lMachineIndependent -lGenericCodeGen -lOSDependent -lSPIRV -L${pkgs.lib.getLib final.spirv-tools}/lib -lSPIRV-Tools-opt -lSPIRV-Tools"
            '';
            postPatch = (o.postPatch or "") + ''
              sed -i \
                -e '/add_library(shaderc_shared SHARED/,/set_target_properties(shaderc_shared PROPERTIES SOVERSION 1)/d' \
                -e 's/TARGETS shaderc shaderc_shared/TARGETS shaderc/' \
                -e '/target_link_libraries(shaderc_shared PRIVATE/d' \
                libshaderc/CMakeLists.txt
            '';
          });

          # nixpkgs' no-shared-libs.patch no longer applies to the new
          # SPIRV-Tools; reimplement it: drop the unconditional shared
          # library target, which cannot link on static musl.
          spirv-tools = prev.spirv-tools.overrideAttrs (o: {
            patches = builtins.filter
              (p: !pkgs.lib.hasSuffix "no-shared-libs.patch" (toString p))
              (o.patches or [ ]);
            postPatch = (o.postPatch or "") + ''
              sed -i \
                -e '/add_library(''${SPIRV_TOOLS}-shared SHARED/,/^)$/d' \
                -e 's/ ''${SPIRV_TOOLS}-shared//g' \
                -e 's/''${SPIRV_TOOLS}-shared/''${SPIRV_TOOLS}-static/g' \
                source/CMakeLists.txt
            '';
          });
        });

        # Static FFmpeg for a given package set (native pkgsStatic or a
        # crossPkgs.pkgsStatic), with an explicit curated feature set
        # (the wader/static-ffmpeg model): decode via the native
        # decoders plus dav1d, encode libx264 + native aac (all the HLS
        # pipeline uses), plain-http network IO for mpv streaming from
        # the loopback server. Everything else is off explicitly, both
        # the options whose static musl builds are broken in nixpkgs
        # (pulseaudio/elfutils chains, dlopen-based loaders, packages
        # that insist on shared libraries) and the encoders/protocols
        # StreamX does not use, so a nixpkgs bump cannot silently
        # re-enable a broken one. Note: no TLS, so the in-process player
        # streams http:// only; https playback falls back to system mpv.
        staticFfmpegFor = ps: (staticSetOf ps).ffmpeg-headless.override {
          withGPL = true;
          withX264 = true;
          withDav1d = true;
          withNetwork = true;
          # encoders/filters the pipeline does not use
          withAom = false; withSvtav1 = false; withTheora = false;
          withVorbis = false; withOpus = false; withMp3lame = false;
          withWebp = false; withOpenjpeg = false; withSpeex = false;
          withX265 = false; withXvid = false; withVidStab = false;
          withZimg = false; withSoxr = false;
          # IO/protocol/hardware extras
          withAlsa = false; withBluray = false; withSsh = false;
          withSrt = false; withGnutls = false; withSamba = false;
          withZvbi = false; withXml2 = false; withDrm = false;
          withAmf = false; withGmp = false;
          withFontconfig = false; withFreetype = false; withFribidi = false;
          withHarfbuzz = false;
          withOpenmpt = false; withV4l2 = false; withV4l2M2m = false;
          withVaapi = false; withVdpau = false; withRist = false;
          withOpencl = false; withOpenapv = false; withVulkan = false;
          withRtmp = false; withCelt = false; withGsm = false;
          withMfx = false; withVpl = false; withVpx = false;
          withJxl = false; withSnappy = false; withCodec2 = false;
          withIlbc = false; withTwolame = false; withUavs3d = false;
          withLcms2 = false; withLc3 = false;
        };

        # Static libmpv for the Linux desktop build. Video output is
        # Vulkan via libplacebo (libGL/x11 GL paths off: libglvnd is
        # dlopen-based and cannot be static); audio backends that need
        # host daemons (pulse, pipewire, jack, openal) are off, ALSA
        # stays. DRM/KMS output needs mesa's gbm (not static-buildable).
        staticPkgs = staticSetOf pkgs;
        staticPlacebo = (staticPkgs.libplacebo.override {
          libGL = null;
          libx11 = null;
          libdovi = null;
        }).overrideAttrs (o: {
          # The inputs nulled above are meson auto-features; make the
          # disable explicit so configure does not require them.
          mesonFlags = (o.mesonFlags or [ ]) ++ [
            "-Dlibdovi=disabled"
            "-Dopengl=disabled"
            "-Dd3d11=disabled"
          ];
        });
        staticMpv = (staticPkgs.mpv-unwrapped.override {
          ffmpeg = staticFfmpegFor pkgs;
          libplacebo = staticPlacebo;
          libGL = null;
          # Plain static lua for the OSC scripts: the luarocks bootstrap
          # (luasocket) does not build statically.
          lua = staticPkgs.lua5_2 // { withPackages = _: staticPkgs.lua5_2; };
          drmSupport = false;
          # vamp-plugin-sdk (via rubberband) cannot build statically;
          # mpv's native scaletempo covers speed changes.
          rubberbandSupport = false;
          # libsndfile (via libbs2b) cannot build against the dynamic
          # host ALSA; the crossfeed filter is nonessential.
          bs2bSupport = false;
          openalSupport = false;
          pipewireSupport = false;
          pulseSupport = false;
          jackaudioSupport = false;
          vaapiSupport = false;
          vdpauSupport = false;
          sdl2Support = false;
          cacaSupport = false;
          vapoursynthSupport = false;
          javascriptSupport = false;
        }).overrideAttrs (o: {
          # Only libmpv.a is consumed (linked into streamx-desktop); the
          # mpv player binary cannot link at all here because the static
          # toolchain refuses the dynamic libvulkan stub.
          mesonFlags = (o.mesonFlags or [ ]) ++ [ "-Dcplayer=false" ];
          doInstallCheck = false;
          # Upstream postInstall/postFixup handle player tools under
          # $out/bin and the doc output; neither exists in a
          # library-only build.
          postInstall = "";
          postFixup = "";
          outputs = pkgs.lib.remove "doc" o.outputs;
        });

        # Release build of the `streamx` server for one target triple.
        # The server has no C library dependencies (rustls, bundled
        # SQLite; FFmpeg is a runtime process), so cross builds only need
        # a C cross compiler for the few `cc`-built crates. musl targets
        # are linked fully static and verified by the linkage check.
        mkServer = { crossPkgs ? null, target ? null, static ? false, embedFfmpeg ? false }:
          let
            embeddedFfmpeg = staticFfmpegFor (if crossPkgs == null then pkgs else crossPkgs);
            base = commonArgs // {
              src = srcWithWeb;
              pname = "streamx";
              cargoExtraArgs = "-p streamx"
                + pkgs.lib.optionalString embedFfmpeg " --features embed-ffmpeg";
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
            embedEnv = pkgs.lib.optionalAttrs embedFfmpeg {
              STREAMX_FFMPEG_BIN = "${pkgs.lib.getBin embeddedFfmpeg}/bin/ffmpeg";
              STREAMX_FFPROBE_BIN = "${pkgs.lib.getBin embeddedFfmpeg}/bin/ffprobe";
            };
            args = base // crossEnv // embedEnv;
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
          # Media dependencies for scripts/release.sh: libmpv (link +
          # bundle) and the ffmpeg/ffprobe executables shipped in the
          # macOS .app. Built per system, so an Apple Silicon host with
          # Rosetta (`extra-platforms = x86_64-darwin`) can fetch the
          # Intel set for cross bundling.
          media-deps = pkgs.buildEnv {
            name = "streamx-media-deps";
            paths = [
              pkgs.mpv-unwrapped
              pkgs.mpv-unwrapped.dev
              pkgs.ffmpeg-full
              (pkgs.lib.getBin pkgs.ffmpeg-full)
            ];
          };
          streamx = mkServer { };
          streamx-linkcheck = linkcheck;
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          # The static libmpv closure on its own, for debugging the
          # desktop link (nix build .#static-libmpv).
          static-libmpv = staticMpv;
          streamx-desktop = craneLib.buildPackage (commonArgs // {
            src = srcWithWeb;
            pname = "streamx-desktop";
            cargoExtraArgs = "-p streamx-desktop";
            doCheck = false;
            # Static libmpv: build.rs probes `pkg-config --static mpv`.
            STREAMX_MPV_STATIC = "1";
            # Host-dynamic system stack first so X11/Wayland/Vulkan
            # resolve to the allowlisted shared libraries, then the
            # static media closure (mpv/FFmpeg/libass/... archives).
            # Reusing the static packages' own buildInputs keeps the
            # pkg-config closure complete without hand-maintaining it.
            # openssl/shaderc are also excluded: their dynamic libraries
            # would win over the static archives at link time and are
            # not in the linkage allowlist (the desktop uses rustls and
            # the static shaderc_combined).
            buildInputs =
              (pkgs.lib.subtractLists
                [ pkgs.mpv-unwrapped pkgs.ffmpeg-full pkgs.openssl pkgs.shaderc ]
                commonBuildInputs)
              ++ [ staticMpv staticPlacebo (staticFfmpegFor pkgs) ]
              ++ staticMpv.buildInputs
              ++ staticPlacebo.buildInputs
              ++ (staticFfmpegFor pkgs).buildInputs;
          });
          # GitHub-release variant of the desktop binary. The raw nix
          # build only starts on Nix systems: its ELF interpreter and
          # RUNPATH point into /nix/store. Rewrite to the standard FHS
          # loader and drop the RUNPATH so a stock distribution's
          # system libraries resolve. Enforced by the linux-dist policy
          # (checks.linkage-desktop-dist).
          streamx-desktop-dist = pkgs.runCommand "streamx-desktop-dist"
            { nativeBuildInputs = [ pkgs.patchelf ]; } ''
            mkdir -p $out/bin
            cp ${packages.streamx-desktop}/bin/streamx-desktop $out/bin/
            chmod +w $out/bin/streamx-desktop
            patchelf \
              --set-interpreter ${if pkgs.stdenv.hostPlatform.isAarch64
                then "/lib/ld-linux-aarch64.so.1"
                else "/lib64/ld-linux-x86-64.so.2"} \
              --remove-rpath \
              $out/bin/streamx-desktop
            chmod -w $out/bin/streamx-desktop

            # Desktop integration: menu entry + icon (hicolor), matched
            # to the window's app_id ("streamx") for taskbar/dock icons.
            mkdir -p $out/share/applications \
                     $out/share/icons/hicolor/512x512/apps \
                     $out/share/icons/hicolor/192x192/apps
            cp ${./assets/linux/streamx-desktop.desktop} \
               $out/share/applications/streamx-desktop.desktop
            cp ${./web/public/icons/android-chrome-512x512.png} \
               $out/share/icons/hicolor/512x512/apps/streamx-desktop.png
            cp ${./web/public/icons/android-chrome-192x192.png} \
               $out/share/icons/hicolor/192x192/apps/streamx-desktop.png
          '';
          streamx-x86_64-linux-musl = mkServer {
            crossPkgs = pkgs.pkgsCross.musl64;
            target = "x86_64-unknown-linux-musl";
            static = true;
            embedFfmpeg = true;
          };
          streamx-aarch64-linux-musl = mkServer {
            crossPkgs = pkgs.pkgsCross.aarch64-multiplatform-musl;
            target = "aarch64-unknown-linux-musl";
            static = true;
            embedFfmpeg = true;
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

            # Release tooling
            git-cliff
            gh

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
            # Same trick for yt-dlp: mpv's ytdl hook needs it to resolve
            # YouTube trailer URLs.
            export STREAMX_YTDLP_BUILD_PATH="${pkgs.yt-dlp}/bin/yt-dlp"
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

        # Command entry points:
        #   nix run .#build-all   every artifact this platform can produce
        #   nix run .#release -- patch|minor|major [--dry-run]
        apps = {
          build-all = {
            type = "app";
            program = pkgs.lib.getExe (pkgs.writeShellApplication {
              name = "streamx-build-all";
              runtimeInputs = [ pkgs.git ];
              text = ''
                cd "$(git rev-parse --show-toplevel)"
                if [ "$(uname -s)" = "Darwin" ]; then
                  nix develop --command scripts/release.sh aarch64-apple-darwin dist/StreamX-aarch64.dmg
                  nix develop --command scripts/release.sh x86_64-apple-darwin dist/StreamX-x86_64.dmg
                else
                  scripts/verify-release.sh
                fi
              '';
            });
          };
          release = {
            type = "app";
            program = pkgs.lib.getExe (pkgs.writeShellApplication {
              name = "streamx-release";
              runtimeInputs = [ pkgs.git pkgs.git-cliff pkgs.gh pkgs.cargo ];
              text = ''
                cd "$(git rev-parse --show-toplevel)"
                exec scripts/release-tag.sh "$@"
              '';
            });
          };
        };

        # cargo clippy + fmt as reusable checks. Run with: nix flake check
        checks = {
          # srcWithWeb: rust-embed derives Asset from web/dist at compile
          # time, and clippy compiles the real crates.
          clippy = craneLib.cargoClippy (commonArgs // {
            src = srcWithWeb;
            cargoArtifacts = craneLib.buildDepsOnly (commonArgs // { src = srcWithWeb; });
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
          linkage-desktop-dist =
            linkageCheck "desktop-dist" packages.streamx-desktop-dist "streamx-desktop" "linux-dist";
        };
      }
    );
}
