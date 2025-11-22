use std::path::{Path, PathBuf};

use anyhow::Result;
use rustyjack_core::{Commands, HandlerResult, dispatch_command, resolve_root};

#[derive(Clone)]
pub struct CoreBridge {
    root: PathBuf,
}

impl CoreBridge {
    pub fn with_root(root: Option<PathBuf>) -> Result<Self> {
        let resolved = resolve_root(root)?;
        Ok(Self { root: resolved })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dispatch(&self, command: Commands) -> Result<HandlerResult> {
        dispatch_command(&self.root, command)
    }
}
