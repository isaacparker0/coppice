# Temporary Migration Note: Functions Coverage Moves

This is a temporary tracking note for coverage intentionally removed from
`unified_tests/declarations/functions` while splitting the old runtime
`functions/minimal_valid` kitchen-sink case.

## Moved out of functions ownership

1. Union/optional behavior coverage:

- legacy behavior: `Maybe[T]`, `coalesce[T](value: Maybe[T], fallback: T)`
- expected destination ownership: union/optional type coverage fixtures
- migration status: TODO

2. General control-flow output checks:

- legacy behavior: `describe(true)` branch output and `if/else` output path
- expected destination ownership: control-flow runtime coverage fixtures
- migration status: TODO

## Still needed within functions ownership

1. Mutable parameter reassignment runtime behavior:

- legacy behavior: `addTwo(mut value)`
- expected destination ownership: functions runtime coverage in this feature
- migration status: TODO (add focused case)

2. Typed local function-value binding behavior:

- legacy behavior: `localMapper: function(int64) -> int64 := plusOne`
- expected destination ownership: functions runtime coverage in this feature
- migration status: TODO (add focused case if not covered elsewhere)

This note should be removed after equivalent focused coverage exists in the
owning feature fixtures.
