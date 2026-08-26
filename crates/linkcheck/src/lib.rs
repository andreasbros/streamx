//! Linkage policy checks for release binaries.
//!
//! StreamX ships self-contained executables. What that means differs
//! per platform, so the policy is explicit:
//!
//! - Linux, musl targets: fully static. No ELF interpreter, no
//!   `DT_NEEDED` entries at all.
//! - Linux, glibc targets (the GPUI desktop app): dynamic against an
//!   allowlist of host system libraries only (libc, the graphics and
//!   windowing stack). Nothing from a Nix store, Homebrew, or a build
//!   directory.
//! - macOS: Apple does not support static executables, so the rule is
//!   "system frameworks only": every `LC_LOAD_DYLIB` must live under
//!   `/System/Library/` or `/usr/lib/`. A bundled dylib is allowed only
//!   through `@rpath/` or `@executable_path/` (shipped inside the .app).

use goblin::mach::{Mach, MachO};
use goblin::Object;
use snafu::{ResultExt, Snafu};
use std::path::Path;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("failed to read {path}: {source}"))]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[snafu(display("failed to parse {path}: {source}"))]
    Parse {
        path: String,
        source: goblin::error::Error,
    },
    #[snafu(display("unsupported object format in {path}"))]
    Unsupported { path: String },
    #[snafu(display("{path} violates the {policy} policy:\n  {}", problems.join("\n  ")))]
    Policy {
        path: String,
        policy: String,
        problems: Vec<String>,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// What a binary links against, normalized across formats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linkage {
    pub format: Format,
    /// ELF `PT_INTERP` (dynamic loader). `None` for static executables
    /// and for Mach-O.
    pub interpreter: Option<String>,
    /// ELF `DT_NEEDED` sonames or Mach-O `LC_LOAD_DYLIB` install names.
    pub libraries: Vec<String>,
    /// ELF `DT_RUNPATH`/`DT_RPATH` entries. Empty for Mach-O.
    pub runpaths: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Elf,
    MachO,
    Pe,
}

/// Linkage rule to enforce.
#[derive(Debug, Clone)]
pub enum Policy {
    /// No interpreter and no shared libraries (musl static builds).
    FullyStatic,
    /// Shared libraries allowed only when their soname (file name) is in
    /// the allowlist. Used for glibc desktop builds whose graphics
    /// stack must come from the host.
    SystemOnly { allowed_sonames: Vec<String> },
    /// macOS rule: dylibs only from Apple system locations or bundled
    /// via `@rpath` / `@executable_path`.
    MacosSystemFrameworks,
    /// macOS development builds: like `MacosSystemFrameworks`, but a
    /// dylib from the build environment is accepted when its basename
    /// is in the bundle manifest, because release packaging ships that
    /// library inside the .app. Anything outside the manifest fails.
    MacosDevBundle { bundle_manifest: Vec<String> },
    /// Linux development builds: like `SystemOnly`, but a soname in the
    /// bundle manifest is accepted because the release build links that
    /// library statically (dev cargo builds link the dynamic one from
    /// the build environment for fast iteration).
    LinuxDevBundle {
        allowed_sonames: Vec<String>,
        bundle_manifest: Vec<String>,
    },
    /// Distributable Linux desktop artifact: `SystemOnly` plus the
    /// executable must be loadable on a stock FHS distribution: the
    /// ELF interpreter must be the standard system loader and no
    /// RUNPATH may reference a build store. A Nix-built binary fails
    /// this until release packaging rewrites it.
    LinuxDist { allowed_sonames: Vec<String> },
    /// Distributable Windows artifact: every imported DLL must be a
    /// Windows system DLL or a library the release zip ships next to
    /// the executable (libmpv). The CRT is static, so no vcruntime
    /// redistributable may appear.
    WindowsDist {
        allowed_dlls: Vec<String>,
        bundle_manifest: Vec<String>,
    },
}

impl Policy {
    pub fn name(&self) -> &'static str {
        match self {
            Policy::FullyStatic => "fully-static",
            Policy::SystemOnly { .. } => "system-only",
            Policy::MacosSystemFrameworks => "macos-system-frameworks",
            Policy::MacosDevBundle { .. } => "macos-dev-bundle",
            Policy::LinuxDevBundle { .. } => "linux-dev-bundle",
            Policy::LinuxDist { .. } => "linux-dist",
            Policy::WindowsDist { .. } => "windows-dist",
        }
    }
}

/// Standard FHS dynamic loaders a distributable Linux binary may use.
pub fn fhs_interpreters() -> Vec<String> {
    ["/lib64/ld-linux-x86-64.so.2", "/lib/ld-linux-aarch64.so.1"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Host libraries a Linux GPUI desktop build may load dynamically.
/// Everything else (mpv, ffmpeg, sqlite, ssl, ...) must be static.
pub fn linux_desktop_allowlist() -> Vec<String> {
    [
        "libc.so.6",
        "libm.so.6",
        "libdl.so.2",
        "libpthread.so.0",
        "librt.so.1",
        "libgcc_s.so.1",
        // C++ runtime for the statically linked C++ media libraries
        // (libplacebo, shaderc, zimg); ships with every glibc distro,
        // same family as libgcc_s.
        "libstdc++.so.6",
        "ld-linux-x86-64.so.2",
        "ld-linux-aarch64.so.1",
        "libvulkan.so.1",
        "libwayland-client.so.0",
        // wayland-client companion, provided by the same host package.
        "libwayland-cursor.so.0",
        "libwayland-egl.so.1",
        "libxkbcommon.so.0",
        // xkbcommon's X11 bridge, same host package as libxkbcommon.
        "libxkbcommon-x11.so.0",
        "libX11.so.6",
        "libX11-xcb.so.1",
        "libxcb.so.1",
        "libXcursor.so.1",
        // Core X11 extension libraries shipped alongside libX11.
        "libXext.so.6",
        "libXfixes.so.3",
        "libXi.so.6",
        "libXrandr.so.2",
        "libfontconfig.so.1",
        "libfreetype.so.6",
        // Host ALSA: audio routing (PipeWire/Pulse plugins) only works
        // through the host's libasound, never a static copy.
        "libasound.so.2",
        "libdbus-1.so.3",
        "libva.so.2",
        "libdrm.so.2",
        "libEGL.so.1",
        "libGL.so.1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Libraries a Linux dev build may load from the build environment
/// because the release build links them statically.
pub fn linux_bundle_manifest() -> Vec<String> {
    ["libmpv"].iter().map(|s| s.to_string()).collect()
}

/// Windows system DLLs (and API-set families) every supported Windows
/// installation provides. Compared case-insensitively.
pub fn windows_system_dlls() -> Vec<String> {
    [
        "kernel32.dll",
        "ntdll.dll",
        "user32.dll",
        "shell32.dll",
        "shlwapi.dll",
        "advapi32.dll",
        "bcrypt.dll",
        "crypt32.dll",
        "ncrypt.dll",
        "secur32.dll",
        "ws2_32.dll",
        "iphlpapi.dll",
        "gdi32.dll",
        "ole32.dll",
        "oleaut32.dll",
        "comctl32.dll",
        "comdlg32.dll",
        "uxtheme.dll",
        "dwmapi.dll",
        "imm32.dll",
        "userenv.dll",
        "winmm.dll",
        "version.dll",
        "setupapi.dll",
        "propsys.dll",
        "runtimeobject.dll",
        // Graphics/text stack used by GPUI's DirectX backend.
        "d3d11.dll",
        "d3dcompiler_47.dll",
        "dxgi.dll",
        "d2d1.dll",
        "dwrite.dll",
        "dcomp.dll",
        "windowscodecs.dll",
        // Media Foundation (hardware decode).
        "mf.dll",
        "mfplat.dll",
        "mfreadwrite.dll",
        // Universal CRT: part of Windows 10+ itself (the MSVC CRT is
        // linked statically; ucrtbase is an OS component).
        "ucrtbase.dll",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Libraries the Windows release zip ships next to the executable.
pub fn windows_bundle_manifest() -> Vec<String> {
    ["libmpv-2.dll", "mpv-2.dll"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn windows_dll_allowed(dll: &str, allowed: &[String], manifest: &[String]) -> bool {
    let d = dll.to_ascii_lowercase();
    // API set schema DLLs (api-ms-win-*, ext-ms-*) are Windows OS
    // forwarders, present on every supported Windows.
    d.starts_with("api-ms-win-")
        || d.starts_with("ext-ms-")
        || allowed.iter().any(|a| a.eq_ignore_ascii_case(&d))
        || manifest.iter().any(|m| m.eq_ignore_ascii_case(&d))
}

/// Third-party libraries the macOS .app bundles next to the binary.
/// A dev build may reference these from the build environment; the
/// release artifact must reference them via `@rpath`.
pub fn macos_bundle_manifest() -> Vec<String> {
    ["libmpv"].iter().map(|s| s.to_string()).collect()
}

fn macos_system_or_bundled(lib: &str) -> bool {
    lib.starts_with("/System/Library/")
        || lib.starts_with("/usr/lib/")
        || lib.starts_with("@rpath/")
        || lib.starts_with("@executable_path/")
        || lib.starts_with("@loader_path/")
}

/// Parse a binary's linkage. Fat Mach-O files report the union of
/// their slices.
pub fn linkage(path: &Path) -> Result<Linkage> {
    let display = path.display().to_string();
    let bytes = std::fs::read(path).context(ReadSnafu { path: &display })?;
    let object = Object::parse(&bytes).context(ParseSnafu { path: &display })?;
    match object {
        Object::Elf(elf) => Ok(Linkage {
            format: Format::Elf,
            interpreter: elf.interpreter.map(|s| s.to_string()),
            libraries: elf.libraries.iter().map(|s| s.to_string()).collect(),
            runpaths: elf
                .runpaths
                .iter()
                .chain(elf.rpaths.iter())
                .flat_map(|p| p.split(':'))
                .filter(|p| !p.is_empty())
                .map(|s| s.to_string())
                .collect(),
        }),
        Object::PE(pe) => {
            let mut libraries: Vec<String> = Vec::new();
            for import in &pe.imports {
                let dll = import.dll.to_string();
                if !libraries.contains(&dll) {
                    libraries.push(dll);
                }
            }
            Ok(Linkage {
                format: Format::Pe,
                interpreter: None,
                libraries,
                runpaths: Vec::new(),
            })
        }
        Object::Mach(Mach::Binary(macho)) => Ok(macho_linkage(&macho)),
        Object::Mach(Mach::Fat(fat)) => {
            let mut libraries: Vec<String> = Vec::new();
            for i in 0..fat.narches {
                if let Ok(goblin::mach::SingleArch::MachO(macho)) = fat.get(i) {
                    for lib in macho_linkage(&macho).libraries {
                        if !libraries.contains(&lib) {
                            libraries.push(lib);
                        }
                    }
                }
            }
            Ok(Linkage {
                format: Format::MachO,
                interpreter: None,
                libraries,
                runpaths: Vec::new(),
            })
        }
        _ => Err(Error::Unsupported { path: display }),
    }
}

fn macho_linkage(macho: &MachO) -> Linkage {
    // goblin lists the binary itself as libs[0] ("self").
    let libraries = macho.libs.iter().skip(1).map(|s| s.to_string()).collect();
    Linkage {
        format: Format::MachO,
        interpreter: None,
        libraries,
        runpaths: Vec::new(),
    }
}

/// Problems a linkage has under a policy; empty when compliant.
pub fn violations(linkage: &Linkage, policy: &Policy) -> Vec<String> {
    let mut problems = Vec::new();
    match policy {
        Policy::FullyStatic => {
            if let Some(interp) = &linkage.interpreter {
                problems.push(format!("has dynamic interpreter {interp}"));
            }
            for lib in &linkage.libraries {
                problems.push(format!("needs shared library {lib}"));
            }
        }
        Policy::SystemOnly { allowed_sonames } => {
            for lib in &linkage.libraries {
                let soname = lib.rsplit('/').next().unwrap_or(lib);
                if !allowed_sonames.iter().any(|a| a == soname) {
                    problems.push(format!("needs non-system library {lib}"));
                }
            }
        }
        Policy::MacosSystemFrameworks => {
            for lib in &linkage.libraries {
                if !macos_system_or_bundled(lib) {
                    problems.push(format!("links dylib outside the system or bundle: {lib}"));
                }
            }
        }
        Policy::MacosDevBundle { bundle_manifest } => {
            for lib in &linkage.libraries {
                if macos_system_or_bundled(lib) {
                    continue;
                }
                let base = lib.rsplit('/').next().unwrap_or(lib);
                let in_manifest = bundle_manifest
                    .iter()
                    .any(|m| base == m.as_str() || base.starts_with(&format!("{m}.")));
                if !in_manifest {
                    problems.push(format!(
                        "links dylib outside the system that release packaging does not bundle: {lib}"
                    ));
                }
            }
        }
        Policy::WindowsDist {
            allowed_dlls,
            bundle_manifest,
        } => {
            for lib in &linkage.libraries {
                if !windows_dll_allowed(lib, allowed_dlls, bundle_manifest) {
                    problems.push(format!(
                        "imports DLL that is neither a Windows system DLL nor bundled: {lib}"
                    ));
                }
            }
        }
        Policy::LinuxDist { allowed_sonames } => {
            match &linkage.interpreter {
                Some(interp) if fhs_interpreters().iter().any(|i| i == interp) => {}
                Some(interp) => problems.push(format!(
                    "interpreter {interp} is not a standard system loader; \
                     the binary will not start on a stock distribution"
                )),
                None => {}
            }
            for rp in &linkage.runpaths {
                if rp.contains("/nix/store") {
                    problems.push(format!("RUNPATH references the build store: {rp}"));
                }
            }
            for lib in &linkage.libraries {
                let soname = lib.rsplit('/').next().unwrap_or(lib);
                if !allowed_sonames.iter().any(|a| a == soname) {
                    problems.push(format!("needs non-system library {lib}"));
                }
            }
        }
        Policy::LinuxDevBundle {
            allowed_sonames,
            bundle_manifest,
        } => {
            for lib in &linkage.libraries {
                let soname = lib.rsplit('/').next().unwrap_or(lib);
                let allowed = allowed_sonames.iter().any(|a| a == soname)
                    || bundle_manifest
                        .iter()
                        .any(|m| soname == m.as_str() || soname.starts_with(&format!("{m}.")));
                if !allowed {
                    problems.push(format!(
                        "needs library that the release build neither allows nor links statically: {lib}"
                    ));
                }
            }
        }
    }
    problems
}

/// The policy a binary built for the running target must satisfy.
pub fn policy_for_current_target() -> Policy {
    if cfg!(target_os = "macos") {
        Policy::MacosDevBundle {
            bundle_manifest: macos_bundle_manifest(),
        }
    } else if cfg!(target_os = "windows") {
        // Dev and release builds link the same way on Windows: the mpv
        // import library resolves to a DLL shipped next to the exe.
        Policy::WindowsDist {
            allowed_dlls: windows_system_dlls(),
            bundle_manifest: windows_bundle_manifest(),
        }
    } else if cfg!(target_env = "musl") {
        Policy::FullyStatic
    } else {
        Policy::LinuxDevBundle {
            allowed_sonames: linux_desktop_allowlist(),
            bundle_manifest: linux_bundle_manifest(),
        }
    }
}

/// Fail unless `path` satisfies `policy`.
pub fn assert_policy(path: &Path, policy: &Policy) -> Result<Linkage> {
    let linkage = linkage(path)?;
    let problems = violations(&linkage, policy);
    if problems.is_empty() {
        Ok(linkage)
    } else {
        Err(Error::Policy {
            path: path.display().to_string(),
            policy: policy.name().to_string(),
            problems,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lk(interp: Option<&str>, libs: &[&str]) -> Linkage {
        Linkage {
            format: Format::Elf,
            interpreter: interp.map(String::from),
            libraries: libs.iter().map(|s| s.to_string()).collect(),
            runpaths: Vec::new(),
        }
    }

    #[test]
    fn fully_static_rejects_interpreter_and_libs() {
        let v = violations(
            &lk(Some("/lib/ld-musl-x86_64.so.1"), &["libc.so"]),
            &Policy::FullyStatic,
        );
        assert_eq!(v.len(), 2);
        assert!(violations(&lk(None, &[]), &Policy::FullyStatic).is_empty());
    }

    #[test]
    fn system_only_allows_allowlist_and_rejects_store_paths() {
        let policy = Policy::SystemOnly {
            allowed_sonames: linux_desktop_allowlist(),
        };
        assert!(violations(&lk(None, &["libc.so.6", "libvulkan.so.1"]), &policy).is_empty());
        let v = violations(
            &lk(
                None,
                &["/nix/store/abc-mpv/lib/libmpv.so.2", "libavcodec.so.61"],
            ),
            &policy,
        );
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn macos_allows_system_and_bundle_only() {
        let policy = Policy::MacosSystemFrameworks;
        let ok = lk(
            None,
            &[
                "/System/Library/Frameworks/Metal.framework/Versions/A/Metal",
                "/usr/lib/libSystem.B.dylib",
                "@rpath/libmpv.2.dylib",
            ],
        );
        assert!(violations(&ok, &policy).is_empty());
        let bad = lk(
            None,
            &[
                "/nix/store/xyz-mpv-0.41.0/lib/libmpv.2.dylib",
                "/opt/homebrew/lib/libavcodec.dylib",
            ],
        );
        assert_eq!(violations(&bad, &policy).len(), 2);
    }

    #[test]
    fn windows_dist_accepts_system_and_bundled_dlls_only() {
        let policy = Policy::WindowsDist {
            allowed_dlls: windows_system_dlls(),
            bundle_manifest: windows_bundle_manifest(),
        };
        let ok = lk(
            None,
            &[
                "KERNEL32.dll",
                "api-ms-win-crt-runtime-l1-1-0.dll",
                "d3d11.dll",
                "libmpv-2.dll",
            ],
        );
        assert!(violations(&ok, &policy).is_empty());
        // A dynamic CRT or a stray dependency DLL must fail: the CRT is
        // static and everything else ships in the zip or not at all.
        let bad = lk(None, &["vcruntime140.dll", "avcodec-61.dll"]);
        assert_eq!(violations(&bad, &policy).len(), 2);
    }

    #[test]
    fn linux_dist_rejects_store_interpreter_and_runpath() {
        let policy = Policy::LinuxDist {
            allowed_sonames: linux_desktop_allowlist(),
        };
        let good = lk(Some("/lib64/ld-linux-x86-64.so.2"), &["libc.so.6"]);
        assert!(violations(&good, &policy).is_empty());
        let nix_interp = lk(
            Some("/nix/store/abc-glibc-2.42/lib/ld-linux-x86-64.so.2"),
            &["libc.so.6"],
        );
        assert_eq!(violations(&nix_interp, &policy).len(), 1);
        let mut store_runpath = good.clone();
        store_runpath.runpaths = vec!["/nix/store/abc-libx11/lib".into()];
        assert_eq!(violations(&store_runpath, &policy).len(), 1);
    }

    #[test]
    fn linux_dev_bundle_accepts_store_libmpv_only() {
        let policy = Policy::LinuxDevBundle {
            allowed_sonames: linux_desktop_allowlist(),
            bundle_manifest: linux_bundle_manifest(),
        };
        let dev = lk(None, &["libc.so.6", "/nix/store/abc-mpv/lib/libmpv.so.2"]);
        assert!(violations(&dev, &policy).is_empty());
        let stray = lk(None, &["libavcodec.so.61"]);
        assert_eq!(violations(&stray, &policy).len(), 1);
        // The strict release policy still rejects the dynamic libmpv.
        let release = Policy::SystemOnly {
            allowed_sonames: linux_desktop_allowlist(),
        };
        assert_eq!(violations(&dev, &release).len(), 1);
    }

    #[test]
    fn macos_dev_bundle_accepts_manifest_only() {
        let policy = Policy::MacosDevBundle {
            bundle_manifest: macos_bundle_manifest(),
        };
        let dev = lk(
            None,
            &[
                "/usr/lib/libSystem.B.dylib",
                "/nix/store/xyz-mpv-0.41.0/lib/libmpv.2.dylib",
            ],
        );
        assert!(violations(&dev, &policy).is_empty());
        let stray = lk(None, &["/nix/store/abc-ffmpeg/lib/libavcodec.61.dylib"]);
        assert_eq!(violations(&stray, &policy).len(), 1);
        // The strict release policy still rejects the store libmpv.
        assert_eq!(violations(&dev, &Policy::MacosSystemFrameworks).len(), 1);
    }

    #[test]
    fn current_executable_parses() {
        let exe = std::env::current_exe().expect("current exe");
        let l = linkage(&exe).expect("parse own binary");
        let expected = if cfg!(target_os = "macos") {
            Format::MachO
        } else {
            Format::Elf
        };
        assert_eq!(l.format, expected);
    }
}
