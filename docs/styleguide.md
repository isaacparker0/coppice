# Styleguide

- Prefer precise, unambiguous names; longer is fine when it adds clarity, but
  avoid length for its own sake. Prefer plain words over internal jargon unless
  the jargon is the standard term for the concept.
- Use full words instead of abbreviations.
- Well-known abbreviations (for example `id`, `url`, `http`) may be used and are
  treated as words for casing.
- Name maps/dicts as `VALUE_BY_KEY`: use singular value names when there is one
  value per key (for example `user_by_id`, `owner_by_repo`) and plural value
  names when each key maps to a collection (for example `users_by_team`,
  `errors_by_file`).
- Use comments when intent is non-obvious; if clear naming and control flow
  already make the code self-evident, comments are usually unnecessary.
- For variables/constants that represent quantities with units, append the
  standard unit abbreviation as a suffix (for example `cache_ttl_s`,
  `request_timeout_ms`, `size_kb`).
- Prefer inlining straightforward logic when it is used in one place; extract a
  helper when it materially improves clarity (for example by encapsulating
  genuinely complex logic behind a clear interface).
- Avoid implicit boolean parameters/returns when their meaning is not obvious
  from the function name/signature; prefer explicit names or a small enum/type.
- Prefer self-evident data types. Avoid opaque composite shapes when a named
  struct makes intent explicit.
- Organize code by clear responsibility boundaries, not convenience.
- Keep module boundaries and dependency directions consistent with concept
  ownership.
- Depend on stable interfaces/data models, not implementation internals.
- Optimize for refactorability: small explicit boundaries, narrow public
  surfaces, and isolated side effects.
- Favor decoupling. Small, focused modules/packages are good; do not optimize
  for fewer files by default.
- Prefer globally unambiguous names for shared/public types. Avoid introducing
  overlapping names that require local import aliasing for disambiguation when a
  clearer unique name is practical.
