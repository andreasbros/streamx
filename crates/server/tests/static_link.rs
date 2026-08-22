//! Linkage policy for the shipped binary. Cargo builds the package's
//! binaries for integration tests and exposes their paths, so this
//! checks the real executable, not the test harness.
//!
//! - Linux musl targets: fully static (no interpreter, no DT_NEEDED).
//! - Linux glibc targets: only allowlisted host libraries.
//! - macOS: Apple system frameworks and bundled (@rpath) dylibs only.

use std::path::Path;
use streamx_linkcheck::{assert_policy, policy_for_current_target};

#[test]
fn shipped_binary_matches_platform_linkage_policy() {
    let bin = Path::new(env!("CARGO_BIN_EXE_streamx"));
    let policy = policy_for_current_target();
    let linkage = assert_policy(bin, &policy).expect("linkage policy");
    eprintln!(
        "{} ({}): {} shared libraries, interpreter={:?}",
        bin.display(),
        policy.name(),
        linkage.libraries.len(),
        linkage.interpreter
    );
}

/// Release pipelines point this at the final artifact (for example a
/// cross-built musl binary that this host cannot execute).
#[test]
fn provided_artifact_matches_policy() {
    let Some(path) = std::env::var_os("STREAMX_LINKCHECK_BIN") else {
        return;
    };
    assert_policy(Path::new(&path), &policy_for_current_target()).expect("artifact linkage policy");
}
