pub mod client;
pub mod auth;
pub mod subscriptions;

pub use client::NostrClient;
pub use auth::{login_with_nsec, get_current_pubkey, is_logged_in};
pub use subscriptions::{subscribe_to_projects, subscribe_to_project_content, subscribe_to_profiles};
