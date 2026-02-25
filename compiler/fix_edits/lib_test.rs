use compiler__fix_edits::{ApplyTextEditsError, TextEdit, apply_text_edits, merge_text_edits};

#[test]
fn merge_text_edits_rejects_overlap() {
    let result = merge_text_edits(&[
        TextEdit {
            start_byte_offset: 2,
            end_byte_offset: 4,
            replacement_text: "x".to_string(),
        },
        TextEdit {
            start_byte_offset: 3,
            end_byte_offset: 5,
            replacement_text: "y".to_string(),
        },
    ]);

    assert_eq!(result.accepted_text_edits.len(), 1);
    assert_eq!(result.rejected_text_edits.len(), 1);
}

#[test]
fn apply_text_edits_applies_in_descending_offset_order() {
    let output = apply_text_edits(
        "abcdef",
        &[
            TextEdit {
                start_byte_offset: 0,
                end_byte_offset: 1,
                replacement_text: "A".to_string(),
            },
            TextEdit {
                start_byte_offset: 5,
                end_byte_offset: 6,
                replacement_text: "F".to_string(),
            },
        ],
    )
    .unwrap();

    assert_eq!(output, "AbcdeF");
}

#[test]
fn apply_text_edits_returns_error_for_overlap() {
    let error = apply_text_edits(
        "abcdef",
        &[
            TextEdit {
                start_byte_offset: 1,
                end_byte_offset: 4,
                replacement_text: "x".to_string(),
            },
            TextEdit {
                start_byte_offset: 3,
                end_byte_offset: 5,
                replacement_text: "y".to_string(),
            },
        ],
    )
    .unwrap_err();

    assert!(matches!(error, ApplyTextEditsError::OverlappingEdit { .. }));
}
