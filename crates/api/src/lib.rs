//! StreamX shared HTTP API: wire types, route path constants, optional client.
//!
//! The server crate depends on this crate for its request/response types.
//! The desktop crate depends on the same types plus the `client` feature for
//! a thin reqwest-based wrapper. The web frontend consumes TypeScript
//! bindings generated from these types via ts-rs when the `ts-export` feature
//! is enabled (`cargo test --features ts-export --test ts_export` in the
//! server crate).

pub mod routes;
pub mod types;

#[cfg(feature = "client")]
pub mod client;
