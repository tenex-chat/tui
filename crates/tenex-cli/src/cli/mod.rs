pub mod client;
pub mod config;
pub mod daemon;
pub mod http;
pub mod protocol;

pub use client::{is_daemon_running, send_command, socket_path};
pub use config::CliConfig;
pub use daemon::run_daemon;
pub use http::{run_server, HTTPServerState};
pub use protocol::CliCommand;
// Note: daemon::socket_path is re-exported via client::socket_path
