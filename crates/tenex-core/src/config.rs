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
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self::new("tenex_data")
    }
}
