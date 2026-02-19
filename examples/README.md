# Examples

Run all commands from workspace root.

### Running an example program

```bash
bazel run --run_in_cwd //compiler/cli:main -- \
  --workspace-root "examples/hello_world" \
  run main.bin.copp
```
