use compiler__fix_edits::TextEdit;

#[derive(Clone, Debug)]
pub struct SafeAutofix {
    pub text_edits: Vec<TextEdit>,
}

impl SafeAutofix {
    #[must_use]
    pub fn from_text_edit(text_edit: TextEdit) -> Self {
        Self {
            text_edits: vec![text_edit],
        }
    }
}
