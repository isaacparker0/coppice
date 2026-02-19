use std::path::Path;

pub fn sanitize_workspace_path(path: &str, session_directory: &Path) -> String {
    let raw_prefix = session_directory.to_string_lossy();
    if raw_prefix.is_empty() {
        return path.to_string();
    }

    let raw = raw_prefix.as_ref();
    let mut sanitized = path
        .replace(&format!("{raw}/"), "")
        .replace(&format!("{raw}\\"), "")
        .replace(raw, "");
    while sanitized.starts_with("./") || sanitized.starts_with(".\\") {
        sanitized = sanitized[2..].to_string();
    }
    sanitized.trim_start_matches(['/', '\\']).to_string()
}
