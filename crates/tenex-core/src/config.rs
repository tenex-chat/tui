use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CoreConfig {
    pub data_dir: PathBuf,
}

impl CoreConfig {
    pub fn new<P: AsRef<Path>>(data_dir: P) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
        }
    }

    /// Get the default data directory path
    /// Priority: $TENEX_CLI_BASE_DIR > ~/.tenex/cli/
    pub fn default_data_dir() -> PathBuf {
        if let Ok(base_dir) = std::env::var("TENEX_CLI_BASE_DIR") {
            return PathBuf::from(base_dir);
        }

        dirs::home_dir()
            .map(|home| home.join(".tenex").join("cli"))
            .unwrap_or_else(|| PathBuf::from(".tenex/cli"))
    }
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self::new(Self::default_data_dir())
    }
}
