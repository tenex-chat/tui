pub mod client;
pub mod auth;

pub use client::NostrClient;
pub use auth::{login_with_nsec, get_current_pubkey, is_logged_in};
