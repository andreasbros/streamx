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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Elf,
    MachO,
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
}

impl Policy {
    pub fn name(&self) -> &'static str {
        match self {
            Policy::FullyStatic => "fully-static",
            Policy::SystemOnly { .. } => "system-only",
            Policy::MacosSystemFrameworks => "macos-system-frameworks",
            Policy::MacosDevBundle { .. } => "macos-dev-bundle",
        }
    }
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
        "ld-linux-x86-64.so.2",
        "ld-linux-aarch64.so.1",
        "libvulkan.so.1",
        "libwayland-client.so.0",
        "libwayland-egl.so.1",
        "libxkbcommon.so.0",
        "libX11.so.6",
        "libX11-xcb.so.1",
        "libxcb.so.1",
        "libXcursor.so.1",
        "libXi.so.6",
        "libXrandr.so.2",
        "libfontconfig.so.1",
        "libfreetype.so.6",
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
        }),
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
    }
    problems
}

/// The policy a binary built for the running target must satisfy.
pub fn policy_for_current_target() -> Policy {
    if cfg!(target_os = "macos") {
        Policy::MacosDevBundle {
            bundle_manifest: macos_bundle_manifest(),
        }
    } else if cfg!(target_env = "musl") {
        Policy::FullyStatic
    } else {
        Policy::SystemOnly {
            allowed_sonames: linux_desktop_allowlist(),
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
