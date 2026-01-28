// UniFFI scaffolding for generating Swift/Kotlin bindings
uniffi::setup_scaffolding!();

pub mod config;
pub mod constants;
pub mod events;
pub mod ffi;
pub mod models;
pub mod nostr;
pub mod runtime;
pub mod search;
pub mod stats;
pub mod store;
pub mod streaming;

// Re-export FFI types at crate root for convenience
pub use ffi::{LoginResult, ProjectInfo, TenexCore, TenexError, UserInfo};
