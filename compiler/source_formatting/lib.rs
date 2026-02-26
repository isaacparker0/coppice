use compiler__fix_edits::TextEdit;

#[must_use]
pub fn canonicalize_source_text(source_text: &str) -> String {
    let normalized_line_endings = source_text.replace("\r\n", "\n").replace('\r', "\n");
    if normalized_line_endings.is_empty() {
        return normalized_line_endings;
    }
    let without_trailing_newlines = normalized_line_endings.trim_end_matches('\n');
    format!("{without_trailing_newlines}\n")
}

#[must_use]
pub fn formatting_text_edits(source_text: &str) -> Vec<TextEdit> {
    let canonical_source_text = canonicalize_source_text(source_text);
    if canonical_source_text == source_text {
        return Vec::new();
    }
    vec![TextEdit {
        start_byte_offset: 0,
        end_byte_offset: source_text.len(),
        replacement_text: canonical_source_text,
    }]
}
