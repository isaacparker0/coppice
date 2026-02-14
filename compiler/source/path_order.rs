use std::cmp::Ordering;
use std::path::Path;

#[must_use]
pub fn path_to_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[must_use]
pub fn compare_paths(left: &Path, right: &Path) -> Ordering {
    path_to_key(left).cmp(&path_to_key(right))
}
