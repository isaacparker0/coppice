use std::path::Path;

#[must_use]
pub fn sanitize_workspace_path(path: &str, session_directory: &Path) -> String {
    let raw_prefix = session_directory.to_string_lossy();
    if raw_prefix.is_empty() {
        return path.to_string();
    }

    let raw = raw_prefix.as_ref().trim_end_matches(['/', '\\']);
    let rootless = raw.trim_start_matches(['/', '\\']);
    let mut sanitized = path.to_string();
    let mut changed = false;

    for prefix in [raw, rootless] {
        if prefix.is_empty() {
            continue;
        }
        for candidate in [
            prefix.to_string(),
            format!("{prefix}/"),
            format!("{prefix}\\"),
        ] {
            let next = sanitized.replace(&candidate, "");
            if next != sanitized {
                changed = true;
                sanitized = next;
            }
        }
    }

    if !changed {
        return path.to_string();
    }

    while sanitized.starts_with("./") || sanitized.starts_with(".\\") {
        sanitized = sanitized[2..].to_string();
    }
    sanitized.trim_start_matches(['/', '\\']).to_string()
}
