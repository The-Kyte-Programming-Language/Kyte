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

## Freeing Heap Memory

Heap memory is **not** automatically freed. Call `free()` when you're done:

```kyte
Vault int x = 42;
print(x);
free(x);    // release the memory
```

```kyte
Vault int[] arr = [10, 20, 30];
print(arr[0]);
free(arr);
```

Forgetting to call `free` is a memory leak. Calling `free` twice is undefined behavior. Be careful.

---

## Why Vault?

Most of the time, stack allocation is exactly what you want. Use `Vault` when:

1. **The data needs to outlive its scope** — e.g., across anchor restarts.
2. **The data is too large for the stack** — large arrays, buffers.
3. **You need pointer semantics** — passing a heap address somewhere.

### Classic use case: persisting state across anchor restarts

```kyte
@main(main) {
    Vault int attempt = 0;   // survives Kill
    attempt += 1;

    if attempt < 5 {
        Kill "retry";
    }

    print("done");
    free(attempt);
}
```

Stack variables (`int attempt = 0`) reset on every restart. `Vault int attempt = 0` only initializes once — on the first run.

---

## Summary

| | Stack | Vault |
|---|---|---|
| Allocation | automatic | `Vault` keyword |
| Deallocation | automatic | `free()` |
| Speed | fastest | malloc overhead |
| Survives `Kill` | no | yes |
| Use for | everything else | persistence, large data |

---

## Tips

- Start with stack. Add `Vault` only when you have a reason.
- In anchors with retry logic, use `Vault` for the counter and stack for everything else.
- The compiler will warn you if a `Vault` variable might be leaking (in future versions).
