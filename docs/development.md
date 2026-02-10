# Development

Run all commands from workspace root.

### Build and test

```bash
bazel build //...
```

```bash
bazel test //...
```

### Running the CLI

```bash
bazel run //compiler/cli:main --run_in_cwd -- check path/to/file.lang0
```
