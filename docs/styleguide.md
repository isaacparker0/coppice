# Styleguide

- Prefer precise, unambiguous names; longer is fine when it adds clarity, but
  avoid length for its own sake. Prefer plain words over internal jargon unless
  the jargon is the standard term for the concept.
- Use full words instead of abbreviations.
- Avoid redundant names/comments; if the name is clear, drop the comment.
- Prefer self-evident data types. Avoid opaque composite shapes (for example
  positional tuples for multi-part keys) when a named struct makes intent
  explicit.
