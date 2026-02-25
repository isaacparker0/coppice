use std::collections::BTreeMap;
use std::path::PathBuf;

use compiler__check_pipeline::{CheckedTarget, check_target_with_workspace_root_and_overrides};
use compiler__reports::CompilerFailure;

pub struct CheckSession {
    workspace_root: Option<String>,
    source_override_by_path: BTreeMap<String, String>,
}

impl CheckSession {
    #[must_use]
    pub fn new(workspace_root: Option<String>) -> Self {
        Self {
            workspace_root: workspace_root.map(|root| normalize_workspace_root(&root)),
            source_override_by_path: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn workspace_root(&self) -> Option<&str> {
        self.workspace_root.as_deref()
    }

    pub fn set_workspace_root(&mut self, workspace_root: Option<String>) {
        self.workspace_root = workspace_root.map(|root| normalize_workspace_root(&root));
    }

    pub fn open_or_update_document(&mut self, path: &str, source: String) {
        self.source_override_by_path
            .insert(path.to_string(), source);
    }

    pub fn close_document(&mut self, path: &str) {
        self.source_override_by_path.remove(path);
    }

    pub fn check_target(&self, path: &str) -> Result<CheckedTarget, CompilerFailure> {
        check_target_with_workspace_root_and_overrides(
            path,
            self.workspace_root.as_deref(),
            &self.source_override_by_path,
        )
    }
}

fn normalize_workspace_root(workspace_root: &str) -> String {
    let workspace_root_path = PathBuf::from(workspace_root);
    if workspace_root_path.is_absolute() {
        return workspace_root.to_string();
    }
    path_to_absolute_string(&workspace_root_path)
}

fn path_to_absolute_string(path: &PathBuf) -> String {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(path)
        .to_string_lossy()
        .to_string()
}
