# Statement Termination

## Decision

No semicolons. Newline-based statement termination via lexer insertion.

**Rule:** The lexer inserts a statement terminator after a line's final token if
that token is a trigger — AND the first token on the next line is an
**identifier** or **statement keyword**. Otherwise, the expression continues.

Trigger tokens (last token on line):

- Identifiers
- Literals (integer, string, boolean)
- `)`, `}`
- Keywords: `return`

Statement-starting tokens (first token on next line):

- Identifiers (`x`, `foo`, `user`)
- Statement keywords (`return`, `if`, `mut`)

If the next line starts with anything else (`.`, `+`, `-`, `*`, `(`, `[`, `and`,
`or`, any operator), the expression continues. No newline is inserted.

---

## Examples

Sequential statements — newline inserted because next line starts with
identifier/keyword:

```
print("hello")
foo(bar)

x := compute()
return x
```

Method chains — next line starts with `.`, so expression continues:

```
result := users
    .filter(|u| u.active)
    .map(|u| u.name)
```

Multi-line arithmetic — next line starts with operator, so expression continues:

```
total := base_price
    + tax
    - discount
    * rate
```

Multi-line boolean — next line starts with `and`/`or`, so expression continues:

```
if user.active
    and user.verified
    and user.age > 18 {
    ...
}
```

Inside parentheses — newlines are always ignored (no insertion):

```
result := some_function(
    arg1,
    arg2,
    arg3,
)
```

---

## Why not semicolons?

Rust uses semicolons to distinguish expressions from statements — the last
expression in a block (without `;`) is the implicit return value. coppice has
explicit `return`, so this use case doesn't apply. Semicolons would be pure
visual noise carrying no semantic weight.

## Why not Go's simpler rule?

Go inserts semicolons based only on the last token of a line, with no
consideration of the next line. This forces trailing-dot method chains
(`users.\n    filter(...)`) and trailing operators (`base_price +\n    tax`).
Trailing dots are ugly and break reading flow.

## Why the two-sided rule works

The key insight: in coppice, only **identifiers** and **statement keywords** can
start a valid statement. No operator, no `(`, no `[`, no `-`, no literal can
begin a statement.

- `x := ...` — identifier
- `foo()` — identifier
- `return x` — keyword
- `if cond { ... }` — keyword

A bare `-x` on a line computes a negation and discards it — the compiler rejects
unused expressions. A bare `(expr)` is equally pointless. So there is no
ambiguity: if the next line starts with something that isn't an identifier or
keyword, it must be continuing the previous expression.

This eliminates the classic semicolon-free ambiguities:

| Pattern           | Parse          | Why                                       |
| ----------------- | -------------- | ----------------------------------------- |
| `foo()\n(bar)`    | `foo()(bar)`   | `(` can't start a statement               |
| `x\n- y`          | `x - y`        | `-` can't start a statement               |
| `foo\n[1, 2]`     | `foo[1, 2]`    | `[` can't start a statement               |
| `foo()\nbar()`    | two statements | `bar` is an identifier → newline inserted |
| `foo()\nreturn x` | two statements | `return` is a keyword → newline inserted  |

## Prior art

- **Go:** Lexer-level insertion based on last token only. Simpler rule, but
  forces trailing-dot/trailing-operator style.
- **Kotlin:** Grammar-integrated newlines with per-operator rules for which
  operators allow continuation. More flexible but complex and inconsistent.
- **Swift:** Spacing-based disambiguation. Elegant but unusual (spacing changes
  semantics).
- **Python:** Newlines always terminate unless inside balanced delimiters.
  Requires parenthesizing multi-line expressions.
- **JavaScript:** Parser-level ASI (automatic semicolon insertion) that tries to
  parse, then backtracks. Widely considered a mistake.

coppice's approach is closest to Go's but uses a two-sided check (last token AND
next token) instead of one-sided (last token only), gaining formatting
flexibility without adding ambiguity.
