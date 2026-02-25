use compiler__source_formatting::{canonicalize_source_text, formatting_text_edits};

#[test]
fn canonicalize_source_text_normalizes_line_endings_and_trailing_newlines() {
    let output = canonicalize_source_text("a\r\nb\r\n\r\n");
    assert_eq!(output, "a\nb\n");
}

#[test]
fn formatting_text_edits_returns_empty_for_already_canonical_text() {
    let edits = formatting_text_edits("a\nb\n");
    assert!(edits.is_empty());
}

#[test]
fn formatting_text_edits_returns_whole_file_replacement_when_not_canonical() {
    let edits = formatting_text_edits("a\r\nb");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].start_byte_offset, 0);
    assert_eq!(edits[0].end_byte_offset, 4);
    assert_eq!(edits[0].replacement_text, "a\nb\n");
}
