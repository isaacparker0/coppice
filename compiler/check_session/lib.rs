use compiler__check_pipeline::{CheckedTarget, check_target_with_workspace_root};
use compiler__reports::CompilerFailure;

pub struct CheckSession {
    workspace_root: Option<String>,
}

impl CheckSession {
    #[must_use]
    pub fn new(workspace_root: Option<String>) -> Self {
        Self { workspace_root }
    }

    pub fn check_target(&self, path: &str) -> Result<CheckedTarget, CompilerFailure> {
        check_target_with_workspace_root(path, self.workspace_root.as_deref())
    }
}
