//! `streamx-linkcheck <binary> [--policy static|linux-desktop|macos|macos-dev]`
//!
//! Exit status 0 when the binary satisfies the policy; prints every
//! violation otherwise. Without `--policy` the rule for the host
//! target applies. Used by the Nix release outputs and CI.

use std::path::PathBuf;
use std::process::ExitCode;
use streamx_linkcheck::{
    assert_policy, linux_desktop_allowlist, policy_for_current_target, Policy,
};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut path: Option<PathBuf> = None;
    let mut policy: Option<Policy> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--policy" => {
                policy = match args.next().as_deref() {
                    Some("static") => Some(Policy::FullyStatic),
                    Some("linux-desktop") => Some(Policy::SystemOnly {
                        allowed_sonames: linux_desktop_allowlist(),
                    }),
                    Some("macos") => Some(Policy::MacosSystemFrameworks),
                    Some("macos-dev") => Some(Policy::MacosDevBundle {
                        bundle_manifest: streamx_linkcheck::macos_bundle_manifest(),
                    }),
                    other => {
                        eprintln!("unknown policy {other:?}; expected static|linux-desktop|macos|macos-dev");
                        return ExitCode::from(2);
                    }
                };
            }
            _ => path = Some(PathBuf::from(arg)),
        }
    }
    let Some(path) = path else {
        eprintln!(
            "usage: streamx-linkcheck <binary> [--policy static|linux-desktop|macos|macos-dev]"
        );
        return ExitCode::from(2);
    };
    let policy = policy.unwrap_or_else(policy_for_current_target);
    match assert_policy(&path, &policy) {
        Ok(linkage) => {
            println!(
                "ok: {} satisfies {} ({} shared libraries)",
                path.display(),
                policy.name(),
                linkage.libraries.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
