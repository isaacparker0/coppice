#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub start_byte_offset: usize,
    pub end_byte_offset: usize,
    pub replacement_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedTextEdit {
    pub text_edit: TextEdit,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergedTextEdits {
    pub accepted_text_edits: Vec<TextEdit>,
    pub rejected_text_edits: Vec<RejectedTextEdit>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyTextEditsError {
    InvalidRange {
        start_byte_offset: usize,
        end_byte_offset: usize,
        source_length_bytes: usize,
    },
    OverlappingEdit {
        previous_end_byte_offset: usize,
        next_start_byte_offset: usize,
    },
}

#[must_use]
pub fn merge_text_edits(text_edits: &[TextEdit]) -> MergedTextEdits {
    let mut sorted_text_edits = text_edits.to_vec();
    sorted_text_edits.sort_by(|left, right| {
        left.start_byte_offset
            .cmp(&right.start_byte_offset)
            .then(left.end_byte_offset.cmp(&right.end_byte_offset))
            .then(left.replacement_text.cmp(&right.replacement_text))
    });

    let mut accepted_text_edits = Vec::new();
    let mut rejected_text_edits = Vec::new();
    let mut last_accepted_end_byte_offset = 0usize;
    let mut has_accepted = false;

    for text_edit in sorted_text_edits {
        if has_accepted && text_edit.start_byte_offset < last_accepted_end_byte_offset {
            rejected_text_edits.push(RejectedTextEdit {
                text_edit,
                reason: "overlapping text edit".to_string(),
            });
            continue;
        }
        last_accepted_end_byte_offset = text_edit.end_byte_offset;
        has_accepted = true;
        accepted_text_edits.push(text_edit);
    }

    MergedTextEdits {
        accepted_text_edits,
        rejected_text_edits,
    }
}

pub fn apply_text_edits(
    source_text: &str,
    text_edits: &[TextEdit],
) -> Result<String, ApplyTextEditsError> {
    let source_length_bytes = source_text.len();
    let mut sorted_text_edits = text_edits.to_vec();
    sorted_text_edits.sort_by(|left, right| {
        left.start_byte_offset
            .cmp(&right.start_byte_offset)
            .then(left.end_byte_offset.cmp(&right.end_byte_offset))
    });

    let mut previous_end_byte_offset = 0usize;
    let mut has_previous = false;
    for text_edit in &sorted_text_edits {
        if text_edit.start_byte_offset > text_edit.end_byte_offset
            || text_edit.end_byte_offset > source_length_bytes
        {
            return Err(ApplyTextEditsError::InvalidRange {
                start_byte_offset: text_edit.start_byte_offset,
                end_byte_offset: text_edit.end_byte_offset,
                source_length_bytes,
            });
        }
        if has_previous && text_edit.start_byte_offset < previous_end_byte_offset {
            return Err(ApplyTextEditsError::OverlappingEdit {
                previous_end_byte_offset,
                next_start_byte_offset: text_edit.start_byte_offset,
            });
        }
        previous_end_byte_offset = text_edit.end_byte_offset;
        has_previous = true;
    }

    let mut output = source_text.to_string();
    for text_edit in sorted_text_edits.iter().rev() {
        output.replace_range(
            text_edit.start_byte_offset..text_edit.end_byte_offset,
            &text_edit.replacement_text,
        );
    }

    Ok(output)
}
