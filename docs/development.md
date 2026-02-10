# Development

Run all commands from workspace root.

### Build and test

```bash
bazel build //...
```

```bash
bazel test //...
```

### Run the CLI

```bash
bazel run //compiler/cli:main --run_in_cwd -- check path/to/file.lang0
```

### Update diagnostic test snapshots

```bash
UPDATE_SNAPSHOTS=1 bazel run //tests/diagnostics:diagnostics_test
```
