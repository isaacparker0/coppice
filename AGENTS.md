- Do not make any code changes until explicitly asked to. By default, you should
  communicate with the user, helping them to discuss, problem solve, ideate,
  plan, etc. If you are asked a question, focus on answering and defer code
  changes until specifically approved.

- This codebase uses Bazel, not Cargo. Cargo is only invoked directly for
  `cargo generate-lockfile`, nothing else. When working on the codebase, use
  commands documented in `@docs/development.md`.

- Disregard code formatting. Never try to manually run code formatters, and
  disregard any unexpected formatting diff that appears as you work. We have
  code formatting for every source file type that will run automatically, you
  should not waste time thinking about it.

- Before making any code changes, always read `@docs/styleguide.md`.
