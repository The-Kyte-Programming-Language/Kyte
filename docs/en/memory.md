# Memory Management

Kyte's memory model is simple: stack by default, heap on demand with `Vault`. What you never do: call `free()`.

---

## Stack (Default)

Every variable lives on the stack unless you say otherwise:

```kyte
int x = 42;          // stack
string name = "hi";  // stack
float f = 3.14;      // stack
```

Stack memory disappears automatically when the variable goes out of scope. Fastest possible allocation, zero GC, zero overhead.

---

## Heap — Vault

Two situations call for the heap:

1. **State that must survive an anchor restart** — Kill discards the stack but not the heap.
2. **Data too large for the stack** — multi-megabyte buffers, long arrays.

Prefix with `Vault`:

```kyte
Vault int counter = 0;
Vault int[] buffer = [1, 2, 3, 4, 5];
Vault string msg = "alive on the heap";
```

Under the hood this calls `malloc`. If allocation fails, the runtime raises an error (NULL check is built in).

---

## Automatic Deallocation

You never call `free()` on a Vault variable. The compiler performs **last-use liveness analysis** at compile time: it finds the last point in the code where each Vault variable is read, then inserts `free()` immediately after.

```kyte
@main(main) {
    Vault int x = 42;
    Vault int[] arr = [1, 2, 3];

    print(x);        // last use of x
    // ← compiler inserts free(x) here

    print(arr[0]);   // last use of arr
    // ← compiler inserts free(arr) here

    print("done");
}
```

What you get for free:

- **No memory leaks** — the compiler frees even when you forget.
- **No use-after-free** — freed right after the last use, so there's nothing to read after.
- **No double-free** — exactly one free point per variable.

### Branching

When the last use is inside an `if/else`, the compiler inserts a free at the end of **both** branches:

```kyte
Vault int val = 100;

if cond {
    print(val);
    // ← compiler: free(val)
} else {
    print(val + 1);
    // ← compiler: free(val)
}
// freed exactly once, whichever branch runs
```

### Early Exits

`Kill`, `break`, and `return` trigger immediate cleanup of all live Vault variables in the current scope. No leaks on early exit:

```kyte
@main(main) {
    Vault int x = 10;
    Vault int y = 20;

    if some_condition {
        Kill "abort";   // x and y are freed before restart
    }

    print(x + y);
    // freed here on normal exit
}
```

---

## When to Use Vault

Most local variables should be stack. Reach for Vault when:

### 1. Persisting state across anchor restarts

```kyte
@main(main) {
    Vault int attempt = 0;   // survives Kill
    attempt += 1;

    if attempt < 5 {
        Kill "retry";
    }

    print(f"done after {attempt} tries");
}
```

`int attempt` (stack) would reset to 0 every restart. Only `Vault int` survives Kill.

### 2. Large data

```kyte
Vault int[] big_buffer = [/* thousands of elements */];
```

Large arrays on the stack risk a stack overflow. Vault puts them on the heap where size isn't a problem.

---

## Summary

| | Stack | Vault |
|---|---|---|
| Declaration | `int x = 0;` | `Vault int x = 0;` |
| Deallocation | automatic (scope exit) | automatic (compiler-inserted) |
| Survives `Kill` | no (resets) | yes |
| Speed | fastest | malloc overhead |
| Use for | almost everything | persistent state, large data |

---

## Best Practices

- **Start with stack.** Add `Vault` only when you have a reason.
- **In retry anchors**, make counters and state variables `Vault`; keep everything else on the stack.
- Variables starting with `_` suppress unused-variable warnings.
