# Memory Management

Kyte gives you explicit control over where memory lives. By default, everything is stack-allocated. When you need the heap, you ask for it with `Vault`.

---

## Stack (Default)

All variables are stack-allocated unless you say otherwise:

```kyte
int x = 42;          // stack
string name = "hi";  // stack
float f = 3.14;      // stack
```

Stack memory is automatically reclaimed when the variable goes out of scope. Fast, deterministic, zero overhead.

---

## Heap with `Vault`

Prefix any variable with `Vault` to heap-allocate it:

```kyte
Vault int x = 42;
Vault int[] arr = [1, 2, 3, 4, 5];
Vault string msg = "hello from the heap";
```

Under the hood, Kyte calls `malloc` and checks for `NULL`. If allocation fails, it's treated as a runtime error.

---

## Automatic Memory Reclamation

Vault variables are **automatically freed by the compiler** — you never call `free()` manually. The compiler performs last-use liveness analysis at compile time to determine exactly where each Vault variable is last read, then inserts the free immediately after that point.

```kyte
@main(main) {
    Vault int x = 42;
    print(x);         // last use of x
    // compiler inserts free(x) here automatically
    int y = 100;
    print(y);
}
```

This means:
- **No memory leaks** from forgetting to free.
- **No use-after-free** from freeing too early.
- **No double-free** — the compiler tracks each variable's free point exactly once.

### Branching

When the last use of a Vault variable is inside an `if/else`, the compiler frees it at the end of **both** branches so both execution paths clean up:

```kyte
Vault int x = 42;
if cond {
    print(x);
    // compiler frees x here
} else {
    print(x);
    // compiler frees x here too
}
```

If there's no `else`, or the last use is inside a loop, `while`, `for`, or `match`, the compiler frees after the entire block — always safe, always correct.

### Early Exits

`Kill`, `break`, and `return` trigger immediate cleanup of any Vault variables still in scope via a safety-net cleanup pass. No leaks on early exit.

---

## Why Vault?

Most of the time, stack allocation is exactly what you want. Use `Vault` when:

1. **The data needs to outlive its scope** — e.g., across anchor restarts.
2. **The data is too large for the stack** — large arrays, buffers.
3. **You need pointer semantics** — passing a heap address somewhere.

### Classic use case: persisting state across anchor restarts

```kyte
@main(main) {
    Vault int attempt = 0;   // heap-allocated, survives Kill
    attempt += 1;

    if attempt < 5 {
        Kill "retry";
    }

    print("done");
    // compiler frees attempt here automatically
}
```

Stack variables (`int attempt = 0`) reset on every restart. `Vault int attempt = 0` only initializes once — on the first run.

---

## Summary

| | Stack | Vault |
|---|---|---|
| Allocation | automatic | `Vault` keyword |
| Deallocation | automatic | automatic (compiler-inserted) |
| Speed | fastest | malloc overhead |
| Survives `Kill` | no | yes |
| Use for | everything else | persistence, large data |

---

## Tips

- Start with stack. Add `Vault` only when you have a reason.
- In anchors with retry logic, use `Vault` for the counter and stack for everything else.
- Variables prefixed with `_` suppress unused-variable warnings; all others are checked.
