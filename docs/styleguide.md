# Styleguide

- Prefer precise, unambiguous names; longer is fine when it adds clarity, but
  avoid length for its own sake. Prefer plain words over internal jargon unless
  the jargon is the standard term for the concept.
- Use full words instead of abbreviations.
- Name maps/dicts as `VALUE_BY_KEY`: use singular value names when there is one
  value per key (for example `user_by_id`, `owner_by_repo`) and plural value
  names when each key maps to a collection (for example `users_by_team`,
  `errors_by_file`).
- Use comments when intent is non-obvious; if clear naming and control flow
  already make the code self-evident, comments are usually unnecessary.
- Prefer self-evident data types. Avoid opaque composite shapes when a named
  struct makes intent explicit.
