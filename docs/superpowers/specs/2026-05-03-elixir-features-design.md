# Elixir-Inspired Features Design Spec

**Date:** 2026-05-03  
**Goal:** Bring three Elixir features into Kyte in a systems-language-appropriate way — zero runtime overhead, clear syntax, no compromises on performance.

**Features:**
1. `>>` Pipe operator
2. `when` Guard clauses in match
3. Struct/enum pattern destructuring in match

**Non-goals:** Array head/tail patterns (`[first, ..rest]`), immutability, list comprehensions, multi-clause functions, dynamic dispatch.

---

## Architecture Overview

All three features are **compile-time transformations only**. The LLVM IR produced is identical to hand-written equivalent code. No new runtime primitives, no new allocations, no overhead.

- Pipe `>>`: syntactic sugar → eliminated at parse time, becomes a `Call` node
- Guard `when`: condition check → branch instruction in existing match codegen  
- Pattern destructuring: field binding → pointer offset access at known compile-time offsets

---

## Feature 1: `>>` Pipe Operator

### Syntax

```kyte
expr >> fn_name
expr >> fn_name(arg1, arg2)
```

The left-hand side is inserted as the **first argument** of the right-hand side call.

### Desugaring (parse time)

| Written | Parsed as |
|---|---|
| `data >> print` | `print(data)` |
| `data >> filter(pred)` | `filter(data, pred)` |
| `a >> f(b) >> g(c)` | `g(f(a, b), c)` |

Transformation happens in the parser — the AST never contains a pipe node. Analyzer and codegen see only `Call` nodes, no changes needed there.

### Example

```kyte
fn clamp(int n, int lo, int hi) -> int {
    if n < lo { return lo; }
    if n > hi { return hi; }
    return n;
}

fn double(int n) -> int { return n * 2; }

@main(main) {
    int x = 42;
    // x >> double >> print  desugars to:  print(double(x))
    x >> double >> print

    // with extra args:  x >> clamp(0, 10)  desugars to:  clamp(x, 0, 10)
    x >> clamp(0, 10) >> print   // prints 10
}
```

`filter`, `map` 같은 고차 배열 함수는 stdlib이 갖춰진 후 파이프와 자연스럽게 연결됩니다. 파이프 자체는 stdlib에 무관합니다.

### Changes

| File | Change |
|---|---|
| `src/lexer.rs` | Add `Token::PipeOp` for `>>` |
| `src/parser/expr.rs` | Handle `>>` in Pratt parser (precedence: lower than calls, higher than assignment); rewrite to `Call` immediately |

---

## Feature 2: `when` Guard Clauses

### Syntax

```kyte
match expr {
    pattern when condition => { body }
    pattern               => { body }
}
```

### Binding Pattern

New pattern `n` (bare identifier, no type annotation) — matches any value of the correct type and binds it to `n` for use in the guard and body.

```kyte
match score {
    n when n >= 90 => { print("A"); }
    n when n >= 80 => { print("B"); }
    n              => { print(f"C: {n}"); }
}
```

Type of `n` is inferred from the match subject — no annotation needed.

### Combining with existing patterns

Guards apply to any pattern, not just bindings:

```kyte
match result {
    Result.Ok(val) when val > 100 => { print("big"); }
    Result.Ok(val)                => { print(f"ok: {val}"); }
    Result.Err                    => { print("fail"); }
}
```

### Codegen semantics

```
for each arm:
  1. test pattern → if no match, jump to next arm
  2. if guard present: evaluate condition → if false, jump to next arm
  3. execute body → jump to after-match
```

No guard = existing behavior unchanged.

### AST Changes

```rust
// Pattern: add Binding variant
pub enum Pattern {
    // ... existing ...
    Binding(String),   // bare name — matches anything, binds to name
}

// Match arms: add Option<Expr> for guard
Stmt::Match {
    expr: Expr,
    arms: Vec<(Pattern, Option<Expr>, Vec<(Stmt, Span)>)>,
    //                  ^^^^^^^^^^^^ NEW: when condition
}
```

### Changes

| File | Change |
|---|---|
| `src/ast.rs` | Add `Pattern::Binding(String)`; add `Option<Expr>` guard field to Match arms |
| `src/lexer.rs` | Add `Token::When` keyword |
| `src/parser/stmt.rs` | After parsing pattern, check for `when` token; parse guard expression |
| `src/analyzer/stmt.rs` | Validate guard is bool-typed; register binding variables in scope before guard + body |
| `src/codegen/stmts.rs` | Emit guard conditional branch after pattern match succeeds |

---

## Feature 3: Struct/Enum Pattern Destructuring

### Syntax

```kyte
match value {
    TypeName { field1, field2 }              => { }  // bind fields to same-name vars
    TypeName { field: literal }              => { }  // match field against literal
    TypeName { field: NestedPattern }        => { }  // nested pattern
    TypeName { field1, field2 } when cond   => { }  // with guard
}
```

### Examples

```kyte
// Basic struct destructuring
match point {
    Point { x, y } when x > 0 => { print(f"right: {x}, {y}"); }
    Point { x, y }             => { print(f"left:  {x}, {y}"); }
}

// Partial field match (only name matters)
match user {
    User { name, active: true } => { print(f"active: {name}"); }
    User { name }               => { print(f"inactive: {name}"); }
}

// Nested — struct field matched against enum variant
match user {
    User { name, status: Status.Banned(why) } => { print(f"{name}: banned — {why}"); }
    User { name, status: Status.Active }       => { print(name); }
}
```

### Field pattern forms

| Syntax | Meaning |
|---|---|
| `{ x }` | bind field `x` to variable `x` |
| `{ x: expr_pattern }` | match field `x` against a nested pattern |
| `{ x: 42 }` | match field `x` == 42 (literal) |
| `{ x: Status.Ok(n) }` | match field `x` against enum variant, bind payload to `n` |

Unmentioned fields are ignored — no need to list every field.

### Codegen semantics

Zero-cost: fields are accessed by compile-time-known struct offsets (pointer arithmetic). No copying, no allocation. Binding a field = alloca + store the field pointer dereference.

For nested patterns: recursive match logic on sub-expressions.

### AST Changes

```rust
pub enum Pattern {
    // ... existing ...
    Binding(String),
    StructDestructure {
        struct_name: String,
        fields: Vec<(String, Option<Box<Pattern>>)>,
        // field name + optional sub-pattern (None = bind to same name)
    },
}
```

### Changes

| File | Change |
|---|---|
| `src/ast.rs` | Add `Pattern::StructDestructure { struct_name, fields }` |
| `src/parser/stmt.rs` | Parse `TypeName { field, field: pattern, ... }` in match arm context |
| `src/analyzer/stmt.rs` | Validate struct exists; validate field names; register bound variables in scope; recursively validate nested patterns |
| `src/codegen/stmts.rs` | Look up struct field index; emit GEP + load for each bound field; recurse for nested patterns |

---

## Implementation Order

These three features are independent and can be built in sequence within one branch:

1. **`>>` pipe** — smallest, touches only lexer + parser, no analyzer/codegen changes
2. **`when` guard** — adds `Pattern::Binding` and guard logic; touches all layers but narrowly
3. **Struct destructuring** — builds on guard infrastructure (same match arm codegen path)

---

## Testing

Each feature needs blackbox tests in `test/blackbox/`:

```
test/blackbox/21_pipe.ky          — basic pipe chains, multi-step
test/blackbox/22_guard.ky         — when on int bindings, enum payloads with when
test/blackbox/23_destructure.ky   — struct destructure, partial fields, nested
test/blackbox/24_combined.ky      — all three together
```
