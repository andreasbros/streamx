//! Link libmpv. `libmpv2-sys` only emits `-lmpv`, so the search path
//! and, for static builds, libmpv's transitive dependencies come from
//! pkg-config here.
//!
//! - Default: dynamic libmpv from the build environment (Nix dev shell).
//!   Release packaging bundles it next to the binary (macOS `@rpath`).
//! - `STREAMX_MPV_STATIC=1`: static libmpv plus everything it needs
//!   (FFmpeg, libass, ...), for fully self-contained Linux binaries.

fn main() {
    println!("cargo:rerun-if-env-changed=STREAMX_MPV_STATIC");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");
    let statik = std::env::var("STREAMX_MPV_STATIC")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    match pkg_config::Config::new()
        .statik(statik)
        .cargo_metadata(true)
        .probe("mpv")
    {
        Ok(lib) => {
            for p in &lib.link_paths {
                println!("cargo:rustc-link-search=native={}", p.display());
            }
        }
        Err(e) => {
            println!(
                "cargo:warning=libmpv not found via pkg-config ({e}); \
                 build inside `nix develop` or set PKG_CONFIG_PATH"
            );
        }
    }
}
