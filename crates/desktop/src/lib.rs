//! StreamX desktop crate as a library. The `streamx-desktop` binary imports
//! from here. Keeping modules in `lib.rs` (rather than inline in `main.rs`)
//! lets integration tests under `tests/` exercise the pure pieces.

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

pub mod app;
pub mod asset_source;
pub mod components;
pub mod keybindings;
pub mod pages;
pub mod playback;
pub mod router;
pub mod runtime;
pub mod state;
pub mod text_input;
pub mod theme;
