# Language Design Specification (Draft)

## Design Goals

Rust's safety guarantees and type-system expressiveness, with TypeScript's
ergonomics and Go's compilation speed. One obvious way to do everything. The
compiler is the linter, the formatter, and the style guide.

**Target domain:** Backend services, application code, CLI tools. Not systems
programming, not OS kernels, not embedded.

**Core tradeoff:** Accept ~80-90% of LLVM -O3 peak performance in exchange for
fast compilation, simple mental model, and minimal annotation burden.

---

## Principles

1. **Explicit over implicit.** Visibility uses `public`, not naming conventions.
   Mutation uses `mut`, not default. Sharing uses `shared`, not default.
2. **One canonical construct per intent.** No syntax aliases, no overlapping
   control-flow models. If two forms solve the same job, keep one and enforce
   it.
3. **Ergonomics without multiplicity.** Ergonomic shorthand is acceptable only
   when it has a unique semantic role and does not introduce a second style for
   the same intent.
4. **The compiler is the linter.** No external formatters, no lint configs, no
   style guides. The compiler enforces canonical forms and provides auto-fix
   where correctness is unambiguous.
5. **Designed for hermetic builds.** The import system, module structure, and
   compilation model are designed to map directly to Bazel's dependency graph.
6. **Minimal annotation burden.** No lifetime annotations, no explicit
   borrow/move markers in common code. Constants require explicit type
   annotations; other local expressions are inferred where unambiguous.

### File Roles Are Language Semantics

A file's suffix defines its role (library, binary entrypoint, test), and the
compiler enforces that role. The filename is the single source of truth for how
code participates in the build.

This is somewhat novel in its completeness, but not in its ingredients. Go's
`_test.go` changes compilation semantics based on filename; Rust's `main.rs` vs
`lib.rs` determines whether a crate produces a binary or a library; TypeScript's
`.d.ts` files are parsed under a different model. The precedent exists — Coppice
simply commits to it as a unified, primary mechanism rather than a one-off
exception.

The usual counterarguments do not apply under Coppice's constraints. We do not
support dual-use "importable and runnable" files, so the Python-style `__main__`
convenience is intentionally excluded. We explicitly value build-system
determinism over build-time ambiguity, so "target type is a build config" is the
wrong model for Coppice. And tooling friction is not a barrier here because the
language and its toolchain are built together from the start.

This fully committed stance reinforces the core principles: one canonical way to
do a thing, explicit over implicit, and a clean mapping to hermetic build
graphs.

### Feature Admission Test

New syntax is accepted only if all of the following are true:

1. It has a **unique semantic role** (not another way to express existing
   intent).
2. It is **compiler-enforceable** (rules can be made deterministic and
   diagnostics can point to one canonical form).
3. It **reduces ambiguity** in real code rather than increasing style choice.
4. It preserves the language's constraint of **one canonical construct per
   intent**.

This is language governance, not just style guidance: when overlap appears, the
compiler should require the canonical form.

---

## Syntax

### Variables

```
x := 42                // immutable binding
mut counter := 0       // mutable binding
x: i64 := 42           // explicit type annotation

// No alternatives. No 'let', 'var', 'const', 'val'.
```

### Constants

```
MAX_RETRIES: int64 := 5
DEFAULT_REGION: string := "us-east-1"

// Annotation is required on every constant declaration.
// MAX_RETRIES := 5  // compile error
```

Rationale:

1. Constants are long-lived declarations and commonly part of package contracts;
   explicit types keep API intent stable and obvious at declaration sites.
2. Requiring annotations on all constants avoids split style rules (`public`
   annotated vs private inferred) and preserves one canonical way to write
   constants.
3. This removes cross-file constant type-inference complexity from package
   contract construction, keeping build semantics more deterministic.

### Functions

```
public function authenticate(username: string, password: string) -> Session | AuthError {
    user := find_user(username)?
    if not password.verify(user.hash) {
        return AuthError { message: "invalid credentials" }
    }
    return Session.new(user)
}
```

- `function` keyword required.
- Type after name, separated by `:`.
- Return type after `->`.
- `return` required. No implicit returns.

### Closures

```
items.map(|x| x * 2)

items.filter(|user| {
    user.active && user.age > 18
})
```

Same semantics as functions, shorter syntax for inline use.

### Types

```
public type User :: struct {
    public name: string
    public email: string
    password_hash: string       // type-private field
}
```

Type declarations use the `type` keyword.

### Methods

```
public type User :: struct {
    name: string

    public function display_name(self) -> string {
        return self.name
    }
}
```

Methods are declared inside struct bodies, not in `impl` blocks.

### Enums And Union Types (Distinct Roles)

```
type Direction :: enum {
    North
    South
    East
    West
}
```

Enums define closed sets of named variants.

Use sites are explicit and namespaced:

```
heading: Direction := Direction.North
```

Union composition remains first-class for composing existing types:

```
type ID :: string | u64

function describe(id: ID) -> string {
    match id {
        s: string => "name: {s}"
        n: u64    => "id #{n}"
    }
}
```

Design rule:

1. `enum { ... }` is the canonical closed-set declaration form.
2. `A | B` composes already-declared types (or builtins) only.
3. The compiler does not implicitly synthesize variants from unresolved union
   members.

### Generics

Square brackets. No turbofish problem.

```
public function max[T: Ord](a: T, b: T) -> T {
    if a > b { return a }
    return b
}

type Map[K: Hash + Eq, V] :: struct {
    entries: List[Entry[K, V]]
}
```

### Pattern Matching

```
function area(s: Shape) -> f64 {
    match s {
        c: Circle => PI * c.radius * c.radius
        r: Rect   => r.w * r.h
        Point     => 0.0
    }
}
```

Exhaustive matching enforced by the compiler.

### Control Flow

One loop construct:

```
for item in items { ... }       // iterate collection
for i, item in items { ... }    // with index
for condition { ... }           // conditional loop
for { ... }                     // infinite loop

// No 'while', 'loop', 'forEach', 'for...of', 'for...in'.
```

Control-flow narrowing:

```
function handle(user: User | nil) {
    if user == nil { return }
    // user is User (non-optional) from here — no unwrap needed
    print(user.name)
}
```

### Strings

One string type. One syntax. Double quotes only. Built-in interpolation.

```
name := "world"
greeting := "hello, {name}"

// No single quotes. No template literals. No raw strings.
// No String vs &str. It's 'string'. Always.
```

---

## Type System

### Structural Typing

Interfaces are structural. No explicit `implements` declaration. If a type has
the required methods, it satisfies the interface.

```
type Printable :: interface {
    function to_string(self) -> string
}

// User satisfies Printable because it has to_string. No declaration needed.
type User :: struct {
    name: string

    function to_string(self) -> string {
        return self.name
    }
}

function print_it(thing: Printable) {
    print(thing.to_string())
}
```

Semantic traits (Hash, Eq, Serialize) use explicit `derives` for opt-in.

### Union Types

```
type Result :: Success | Failure
type StringOrInt :: string | u64
```

Tagged unions under the hood. Composable at the use site.

`|` is composition, not enum declaration syntax. Union members must resolve to
existing named types or builtins.

### Intersection Types

```
type Timestamped :: interface {
    created_at: Time
    updated_at: Time
}

type Authored :: interface {
    author: string
}

function fetch_posts() -> List[Timestamped & Authored] { ... }
```

### No Literal Singleton Types

Literal singleton types are intentionally not part of the language.

```
// type Direction :: "north" | "south" // compile error
```

Use explicit enum declarations for closed sets:

```
type Direction :: enum {
    North
    South
    East
    West
}
```

Rationale:

1. Literal singleton types would introduce overlapping ways to model closed
   value sets (`"foo" | "bar"` vs explicit enums), violating the one-way design
   principle.
2. Explicit enums communicate domain intent better than ad-hoc literal unions
   and remain easier to refactor safely across package boundaries.
3. Excluding literal singleton types keeps type inference, assignability, and
   diagnostics simpler and more predictable.

### Nullability

```
function find_user(id: u64) -> User | nil {
    ...
}

user := find_user(42) ?? return
```

`T | nil` is the optional form. Control-flow narrowing eliminates the need for
explicit unwrapping in most code.

### No Implicit Conversions

```
x: i32 := 42
y: i64 := x            // compile error
y: i64 := x.to_i64()   // explicit
```

---

## Generics: Compilation Strategy

Generics compile via **witness tables** (Swift model), not monomorphization.

One function is compiled per generic definition. Type-specific operations
dispatch through a witness table — a struct of function pointers describing how
to copy, destroy, compare, and operate on the type.

Small types (up to ~24 bytes) are stored inline in a fixed-size buffer with zero
heap allocation. Large types spill to the heap.

**Tradeoffs vs monomorphization:**

- Compile time: dramatically faster (one copy per generic function, not N).
- Runtime: ~5-20% slower for generic code due to indirect calls.
- The compiler may opportunistically specialize hot generic functions as an
  optimization. This is optional, not required for correctness.

Type system expressiveness is fully independent of the compilation strategy.
Constraints, associated types, conditional conformance — all resolved at compile
time with zero codegen cost.

Selective monomorphization available via annotation for performance-critical
code:

```
#[specialize]
function dot_product[T: Numeric](a: List[T], b: List[T]) -> T { ... }
```

---

## Memory Model

### Values by Default

All types are values. Assignment copies. No implicit aliasing.

```
p1 := Point { x: 1, y: 2 }
p2 := p1    // copy — p1 and p2 are independent
```

The compiler optimizes this:

- Read-only function parameters: passed by reference automatically.
- Last use of a value: moved, not copied.
- Actual copy only when semantically necessary (mutate + continued use).

This means values-by-default has near-zero overhead in practice. Most "copies"
are elided.

### Immutable by Default

```
x := 42              // immutable
mut y := 0           // mutable — explicit

function transform(data: List[i32]) -> List[i32] {
    // data.push(1)     ← compile error: data is immutable
    return data.map(|x| x * 2)
}

function add_item(mut list: List[i32], item: i32) {
    list.push(item)
}
```

Mutation is visible at the call site:

```
mut items := [1, 2, 3]
add_item(mut items, 4)    // 'mut' required — caller acknowledges mutation
```

### Shared References (When Needed)

Explicit, reference-counted. For the rare case where multiple owners need the
same data.

```
pool := shared ConnectionPool.new(size: 10)

handler1 := Handler { pool: pool }
handler2 := Handler { pool: pool }
// Both reference the same pool. Reference-counted.
```

`shared` values use automatic reference counting (ARC) with deterministic
cleanup. Cycle prevention via `weak` references.

### No Borrow Checker

Ownership is managed through values (stack-scoped, no aliasing) and ARC (for
shared data). No lifetime annotations. No `'a`. No borrow checker fights.

**Safety guarantees without a borrow checker:**

| Property              | Mechanism                                                        |
| --------------------- | ---------------------------------------------------------------- |
| No use-after-free     | ARC for shared, scope-bound for values                           |
| No data races         | Immutable by default, `mut` is exclusive, `shared` requires sync |
| No null dereference   | `T \| nil` with control-flow narrowing                           |
| No unhandled errors   | `T \| Error` union types                                         |
| Deterministic cleanup | ARC, not GC                                                      |
| No aliasing bugs      | Values by default                                                |

---

## Error Handling

Errors are values, expressed as union return types.

```
public function read_file(path: string) -> string | IOError {
    data := fs.read(path)?      // propagate on error
    return data.to_string()
}
```

`?` propagates errors. Exhaustive `match` handles them.

```
match read_file("config.toml") {
    content: string  => print(content)
    e: IOError       => print("failed: {e}")
}
```

No exceptions. No `throw`. No `try/catch`. Every error path is visible in the
type signature.

For unrecoverable invariant violations, use `abort("message")`.

```
if state != "ready" {
    abort("internal invariant violated: state must be ready")
}
```

`abort` is process-terminating and non-recoverable. It is not a substitute for
returning recoverable errors in function signatures.

---

## Package System

### Package Definition

A directory is a package if and only if it contains a `PACKAGE.coppice` file.
Without one, a subdirectory's files belong to the parent package.

All source files must be owned by a package, including role-specialized files:
`*.bin.coppice` and `*.test.coppice` are package members, not standalone
compilation units.

If a `.coppice` source file has no ancestor `PACKAGE.coppice` up to workspace
root, compilation fails.

Why this policy:

1. keeps one canonical build/analysis unit (the package)
2. preserves deterministic mapping to Bazel package targets
3. avoids split semantics between "package mode" and ad-hoc "single-file mode"

```
platform/
  auth/
    PACKAGE.coppice           # makes auth/ a package
    token.coppice             # library file in auth package
    password.coppice          # library file in auth package
    server.bin.coppice        # binary entrypoint file in auth package
    token.test.coppice        # test file in auth package
    crypto/
      encrypt.coppice         # NO PACKAGE.coppice → still part of auth package
      hash.coppice
    oauth/
      PACKAGE.coppice         # makes oauth/ its own package
      google.coppice
```

### PACKAGE.coppice

Contains only a doc comment and `exports` declarations. No code.

```
// platform/auth/PACKAGE.coppice

// Package auth provides authentication and authorization.

exports { Token, parse, hash, verify }
```

`exports` declares selected symbols as the package's external API. This is the
only place `exports` is allowed. The keyword `export` is invalid.

The plural keyword is intentional: `PACKAGE.coppice` is a declarative API table,
not a file-local export statement and not a barrel forwarding module.

### Imports

One import form only: fully qualified package path plus explicit member list. No
relative imports. No glob imports. No conditional imports.

Import paths are always import-origin-prefixed. There is no file-relative or
directory-relative resolution.

Valid path forms:

- `workspace` (workspace-root package)
- `workspace/<first-party-package-path>`
- `std/<stdlib-package-path>`
- `external/<registry-package-path>`

`workspace` denotes the workspace-root package. `workspace/<...>` denotes a
first-party package directory containing `PACKAGE.coppice`. `PACKAGE.coppice`
itself is never written in import syntax.

```
import workspace/platform/auth { Token, parse }
import workspace/platform/auth/oauth { GoogleClient }
import workspace { AppConfig }
import std/fmt { printLine as print }
import external/registry/uuid { V7 }

// import ../auth          ← compile error
// import workspace/*      ← compile error
// import workspace/auth   ← compile error (missing explicit members)
```

Reserved top-level import origin prefixes:

- `workspace` for first-party workspace packages.
- `std` for standard library.
- `external` for third-party dependencies.

Import declarations must appear before top-level declarations in each file.

### Visibility

Two visibility axes:

- Top-level declarations:
  - No modifier — file-private.
  - `public` — package-visible (importable by other files in the same package).
  - External visibility — requires `public` plus `exports` in `PACKAGE.coppice`.
- Struct members:
  - No modifier — type-private (accessible only inside methods on that type).
  - `public` — accessible anywhere the type itself is accessible.

`public` is contextual by declaration kind:

- On top-level declarations: eligible to be imported from other files in the
  same package.
- On struct members: accessible wherever values of that type are accessible.
- Diagnostics must name the contextual meaning in each error.

```
// auth/token.coppice

public type Token :: struct {        // package-visible, can be listed in exports
    public user_id: i64         // visible on the struct externally
    signature: string           // type-private field
}

public function validate(t: Token) -> bool {   // package-visible function
    ...
}
```

Test files (`*.test.coppice`) may import library symbols per normal visibility
rules, but may not declare `public` symbols and are not importable.

### Intra-Package Access

Intra-package usage is explicit for code files. Files do not see sibling
declarations implicitly; they import package-visible (`public`) symbols
explicitly using the same import form as other code files.

`PACKAGE.coppice` is a declarative manifest, not a normal code scope. It does
not import symbols; `exports { ... }` resolves members against package-level
`public` declarations.

```
// auth/token.coppice
public function validate(t: Token) -> bool { ... }

// auth/password.coppice
import workspace/platform/auth { validate, Token }

function check(pw: string, t: Token) -> bool {
    validate(t)    // fine — same package
    ...
}

// auth/crypto/encrypt.coppice
function encrypt(data: string) -> string { ... }   // file-private
```

---

## Testing

### Test Files

Tests live in separate `*.test.coppice` files. Same directory as the source.
`public` declarations are forbidden in test files, and test files are not
importable.

```
auth/
  token.coppice
  token.test.coppice
  password.coppice
  password.test.coppice
```

### Syntax

```
// token.test.coppice

group Token.parse {
    test "handles valid JWT" {
        token := parse("abc.def.ghi")
        assert token.header == "abc"
    }

    test "rejects malformed input" {
        result := parse("garbage")
        assert result matches ParseError
    }
}

group Token.validate {
    test "accepts unexpired token" {
        token := make_test_token(ttl: 3600)
        status := token.validate()
        assert status matches OK
    }
}
```

- `test` blocks with string names.
- `group` blocks for organization. One level of nesting only — the parser
  rejects nested groups.
- `test` blocks can exist outside groups for small files.

### Assertions

One assertion primitive: `assert`. The compiler introspects the expression to
produce detailed failure messages.

```
assert user.age > 18

// Failure output:
//   assert user.age > 18
//          |        |
//          15       18
```

No assertion libraries. No `assertEqual`, `expect().toBe()`. Just `assert`.

### Fixtures

Functions. No framework, no decorators, no dependency injection.

```
// testutil/auth.coppice (with PACKAGE.coppice listing these in exports)

public function make_token(user_id: i64) -> Token {
    return Token.new(user_id: user_id, secret: "test-secret", ttl: 3600)
}
```

```
// token.test.coppice
import testutil/auth { make_token }

test "token contains user id" {
    token := make_token(42)
    assert token.user_id == 42
}
```

Cleanup is handled by deterministic resource cleanup (ARC + destructors). No
`teardown`, `afterEach`, or `yield`.

### Test Output

```
$ coppice test platform/auth/

platform/auth/token.test.coppice
  Token.parse
    ok   handles valid JWT (1ms)
    ok   rejects malformed input (0ms)
  Token.validate
    ok   accepts unexpired token (2ms)
    FAIL rejects expired token (1ms)

  FAIL: "rejects expired token"
    assert status matches TokenExpired
           |      |
           OK     TokenExpired
    at: token.test.coppice:28

3 passed, 1 failed
```

---

## Compiler Strictness

### Enforced Rules (Errors, Not Warnings)

- Unused variables → error (use `_` to discard).
- Unused imports → error.
- Unused function parameters → error (use `_name` to acknowledge).
- Unreachable code → error.
- Non-exhaustive match → error.
- No implicit type conversions.
- Constant declarations must include explicit type annotations.
- Unformatted code → error (in strict mode).

### Naming Conventions (Compiler-Enforced)

- Types: `PascalCase`.
- Functions and variables: `camelCase`.
- Module-level constants: `SCREAMING_SNAKE_CASE`.
- Acronyms follow casing rules: `HttpServer`, not `HTTPServer`.

### One Way to Do Things

No syntax alternatives. No feature overlaps.

- One variable binding form: `:=` / `mut :=`.
- One loop: `for`.
- One string syntax: double quotes with `{}` interpolation.
- One equality operator: `==` (structural).
- One null value: `nil`.
- One generic syntax: `[T]`.
- One optional form: `T | nil`.
- One error propagation: `?`.
- One fatal unrecoverable failure construct: `abort(...)`.
- One union branching form: `match`.
- One union boolean membership check: `matches`.
- One constrained value-set declaration form: `enum { ... }` (not literal
  singleton types).
- One union composition form: `A | B` over already-declared types or builtins.
- The compiler rejects equivalent non-canonical patterns (for example, boolean
  membership `match` expressions where `matches` is the canonical form).

### What Doesn't Exist

- No semicolons (grammar doesn't have them).
- No exceptions / `throw` / `try-catch`.
- No `panic` keyword (use `abort(...)`).
- No `null` AND `undefined` (one `nil`).
- No operator overloading (or very limited).
- No variadic arguments (pass a list).
- No literal singleton types (`"foo"`, `1`, `true` as types).
- No implicit enum-variant synthesis from unresolved union members.
- No implicit returns.
- No single-statement braceless `if`.
- No macros that perform I/O or read files.
- No build scripts.

---

## Toolchain

Single binary. All capabilities built in.

```
coppice build .        # compile (strict: rejects unformatted code)
coppice build --draft  # auto-fix then compile (development mode)
coppice check .        # type-check only, no codegen (fastest feedback)
coppice fix .          # auto-fix all fixable issues
coppice fmt .          # format only (subset of fix)
coppice test .         # run tests
coppice lsp            # language server
coppice doc .          # generate documentation
```

### Fix Mode

`coppice fix` auto-corrects everything with exactly one correct fix:

- Formatting, import sorting, unused import removal, missing trailing commas,
  wrong naming convention (rename across file), unnecessary type annotations.

It does NOT fix ambiguous issues: unused parameters, unreachable code, type
errors, non-exhaustive matches.

### Formatter

Non-configurable. No options file. One canonical output for any valid program.
Built into the compiler, not a separate tool.

### Build Modes

- `coppice build .` — strict. Rejects unfixed code. Used in CI.
- `coppice build --draft .` — runs `fix` implicitly before compiling. Used
  during development.
- `coppice check .` — type-check only, no codegen. Used by LSP for real-time
  feedback. Target: <100ms incremental.

Command invocation policy:

- Commands are run from workspace root.
- Workspace root is explicit:
  - default: current working directory
  - override: `--workspace-root <path>`
- A valid workspace root must contain `PACKAGE.coppice` at its root.
- Invoking with an invalid workspace root is a compile-time error.
- `check` (no path) is canonical and equivalent to `check .`.
- `check <path>` accepts a file or directory path resolved relative to workspace
  root.
- If `<path>` is a source file (including `.bin.coppice`/`.test.coppice`), the
  compiler resolves its owning package and checks that package.
- If a source file has no owning package, `check` fails with a package ownership
  error.

Intent:

- one canonical default (`check`)
- explicit workspace boundary (no implicit root searching)
- deterministic build graph scope

---

## Compilation

### Backend

Cranelift (Rust-native compiler backend). Fast compilation, good-enough output
(~80-90% of LLVM -O3). Optional LLVM backend for release-optimized builds if
ever needed.

### Compilation Units

Parsing is file-level and independent. Typechecking and visibility resolution
are package-level. This preserves granular incremental work while keeping
cross-file semantics explicit and deterministic.

- Granular caching (change one file, recompile one file).
- Parallelism (files compile in parallel).
- Fast incremental builds.

### Deterministic Output

- No timestamps in output.
- No absolute paths in output (use exec-root-relative paths).
- Deterministic iteration order in compiler internals.
- No dependency on environment variables.

Byte-identical output across machines given identical inputs.

---

## Build System Integration (Bazel)

### Gazelle

The source code contains enough information for the build graph to be
mechanically derived.

Gazelle plugin logic:

1. Walk directory tree. A directory with `PACKAGE.coppice` is a package target.
2. Collect package files under that root:
   - `*.coppice` excluding `*.bin.coppice`, `*.test.coppice`, and
     `PACKAGE.coppice` as library source files.
   - include `PACKAGE.coppice` manifest in package metadata.
3. Collect `*.bin.coppice` files → `lang0_binary` targets.
4. Collect `*.test.coppice` files → `lang0_test` targets.
5. Parse `import` statements and map import-origin-prefixed import path to
   target deps.

No heuristics. No configuration file. No import resolution algorithm.

### Target Mapping

```
# One package root (with PACKAGE.coppice) = one lang0_library target

lang0_library(
    name = "auth",
    srcs = [
        "token.coppice",
        "password.coppice",
        "crypto/encrypt.coppice",     # subdir without PACKAGE.coppice
        "crypto/hash.coppice",
    ],
    manifest = "PACKAGE.coppice",
    deps = [
        "//platform/auth/oauth",
        "@lang0_std//time",
    ],
    visibility = ["//visibility:public"],
)

lang0_binary(
    name = "auth_server",
    src = "server.bin.coppice",
    deps = [":auth"],
)

lang0_test(
    name = "auth_test",
    srcs = [
        "token.test.coppice",
        "password.test.coppice",
    ],
    deps = [":auth"],
)
```

### Hermeticity

- Compiler is a single hermetic binary. No system dependencies.
- No build scripts or build-time code execution.
- No environment variable sniffing.
- No macros that read files or perform I/O.
- Fully qualified imports with no resolution ambiguity.
- Remote caching works by default. Remote execution works by default.

### Why the Language Design Helps Bazel

- File-level compilation units → granular action caching.
- Deterministic output → remote cache hits across machines.
- No hidden dependencies → build graph is correct by construction.
- `PACKAGE.coppice` as manifest plus file-role suffixes (`.bin`, `.test`) keeps
  Gazelle plugin logic deterministic and small.
- No transitive header includes, no implicit prelude (or a fixed one) → `deps`
  is minimal and precise.

---

## Concurrency (Sketch)

Structured concurrency. Immutable data shares freely. Mutable data moves.
`shared` data requires explicit synchronization.

```
async {
    a := spawn fetch("url1")
    b := spawn fetch("url2")
    return merge(a.await, b.await)
}
```

`Send`/`Sync`-like constraints inferred by the compiler, not annotated by the
programmer.

Detailed concurrency design is deferred.

---

## Prelude (Fixed)

Always available without import. Not configurable.

```
// Primitive types
i8, i16, i32, i64
u8, u16, u32, u64
f32, f64
bool
string

// Collections
List, Map, Set

// Built-ins
assert, abort, print, nil
```

Everything else requires an explicit import.

---

## Implementation Strategy

1. **Phase 1: Transpile to Rust.** Validate language design. Iterate on syntax
   and type system. Parser + type checker emitting Rust code.
2. **Phase 2: Cranelift backend.** Direct compilation once the language
   stabilizes. Fast compilation for development.
3. **Phase 3 (optional): LLVM backend.** For release-optimized builds if peak
   performance is needed.

The compiler is written in Rust. Parser is hand-written recursive descent.

---

## Serialization and Validation

Part of the standard library (`std/encoding`). Serialization is too fundamental
to leave to ecosystem fragmentation.

### Encoding/Decoding Known Types

```
import std/encoding/json

message := json.decode[Message](raw_bytes)
// Returns Message | DecodeError
```

```
output := json.encode(message)
// Returns string. Always succeeds for valid types.
```

The compiler generates encode/decode implementations from struct definitions. No
derive macros, no decorators, no schema classes.

### Validation of Untrusted Data

Same function, stricter mode for API boundaries:

```
message := json.decode[Message](request.body, validate: true)
// Rejects unknown fields, validates constraints
// Returns Message | ValidationError with structured error details
```

### Type-Level Constraints

Validation constraints are part of the type definition via `where` clauses:

```
public type SignupRequest :: struct {
    email: string where match("[^@]+@[^@]+")
    age: u32 where 13 <= self <= 150
    username: string where 3 <= self.length <= 20
    password: string where self.length >= 8
}
```

`where` clauses are checked at decode time when `validate: true`. Zero cost when
not validating (internal data). Compiled into the type's witness table.

```
result := json.decode[SignupRequest](body, validate: true)
match result {
    req: SignupRequest => handle_signup(req)
    e: ValidationError => {
        // e.fields == [
        //   { field: "email", message: "must match pattern [^@]+@[^@]+" },
        //   { field: "age", message: "must be >= 13, got 5" },
        // ]
        return Response.bad_request(e.fields)
    }
}
```

### Multiple Formats

Consistent interface across formats. Common formats ship in stdlib:

```
import std/encoding/json
import std/encoding/toml
import std/encoding/yaml

json.decode[Config](data)
toml.decode[Config](data)
yaml.decode[Config](data)
```

Niche formats (MessagePack, CBOR, Avro) are third-party libraries implementing
the same `Encoder`/`Decoder` interface.

---

## Self-Hosting

The compiler is written in Rust. Self-hosting (rewriting the compiler in
coppice) is a non-goal for the foreseeable future.

Rust is an excellent language for writing compilers — enums for AST nodes,
pattern matching, strong type system. Coppice targets backend services, not
compiler internals. Using Rust for the compiler is the right tool for the job,
not a compromise.

Self-hosting becomes worth considering only when:

- The language spec is stable (not changing weekly).
- Coppice has proven itself on other large codebases.
- The bootstrap chain can be maintained (CI builds version N from version N-1).
- There's a credibility or contributor-onboarding reason to do it.

TypeScript's compiler moving from TypeScript to Go is a useful reminder that
dogfooding is a means, not an end.

---

## Prior Art and Influences

| Influence      | What's borrowed                                                                 |
| -------------- | ------------------------------------------------------------------------------- |
| **Rust**       | Safety guarantees, `?` error propagation, exhaustive matching, `mut`            |
| **Go**         | Compilation speed, `for` as only loop, package = directory, enforced formatting |
| **TypeScript** | Structural typing, union/intersection types, control-flow narrowing             |
| **Swift**      | Witness table generics, ARC memory model, value semantics                       |
| **Kotlin**     | `val`/`var` distinction (our `:=`/`mut :=`), null safety                        |

---

## Non-Goals

- Systems programming (OS kernels, drivers, allocator control).
- Zero-cost abstractions at all costs.
- Backward compatibility with any existing language.
- Gradual typing or dynamic escape hatches (`any`).
- Macros or metaprogramming (at least initially).
- Redundant expressivity: multiple interchangeable ways to encode the same
  intent.
