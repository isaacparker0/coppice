- Do not make any code changes until explicitly asked to. By default, you should
  communicate with the user, helping them to discuss, problem solve, ideate,
  plan, etc. If you are asked a question, focus on answering and defer code
  changes until specifically approved.
- This codebase uses Bazel, not Cargo. Cargo is only invoked directly for
  `cargo generate-lockfile`, nothing else. When working on the codebase, use
  commands documented in `@docs/development.md`.
- Never run Bazel commands in parallel. Execute Bazel commands sequentially to
  avoid output-base lock contention.
- Disregard code formatting, and never try to manually run code formatters.
- Before making any code changes, always read `@docs/styleguide.md`.
