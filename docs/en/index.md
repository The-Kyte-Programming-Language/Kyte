# Kyte

> Fast, practical, and crash-resistant.

Kyte is a statically-typed language that compiles to native code via LLVM. It's for developers who want C/Rust-level performance without the borrow checker, and without drowning functions in exception-handling boilerplate.

The core idea is **Anchor**: when something goes wrong, your program doesn't crash — it restarts. That logic is built into the language, not bolted on.

---

## Quick Look

```kyte
struct Counter {
    int value;
}

@main(main) {
    Counter c = Counter { value: 0 };

    while c.value < 5 {
        print(c.value);
        c.value += 1;
    }
}
```

```
0
1
2
3
4
```

`@main(main)` — that's Kyte's entry point. The `main` is the anchor kind. Why not just `main()` like a normal function? Because anchors aren't normal functions. They can restart, receive events, and carry their own recovery logic.

---

## Why Kyte?

### Crash? Restart instead.

The most annoying thing about server code: one bad request kills the whole process. Kyte's Anchors solve this at the language level.

```kyte
@main(main) {
    Vault int attempts = 0;
    attempts += 1;

    if attempts < 3 {
        Kill "connection failed — retrying";
    }

    print(f"succeeded after {attempts} tries");
}
```

```
1
2
succeeded after 3 tries
```

`Kill` restarts the anchor. `Vault` keeps the counter alive across restarts. No try/catch pyramid. No supervisor process. Just two keywords.

### Heap allocation without the headache

Kyte tracks where each heap variable is last used and inserts `free()` automatically at exactly that point.

```kyte
@main(main) {
    Vault int[] buffer = [1, 2, 3, 4, 5];
    print(buffer[0]);   // last use
    // compiler inserts free(buffer) here
    print("done");
}
```

### Predictable performance

No GC. No runtime. LLVM compiles it directly to native code, equivalent to hand-written C.

---

## Docs Overview

| Section | What you'll learn |
|---|---|
| [Types & Variables](types.md) | int, float, string, auto inference, arrays, constants |
| [Functions](functions.md) | fn, closures, generics |
| [Control Flow](control-flow.md) | if/else, for, while, loop, match, break, continue |
| [Structs & Enums](structs-enums.md) | Custom types, payload enums |
| [Traits & Impl](traits-impl.md) | Polymorphism, per-type methods |
| [Modules](modules.md) | mod blocks, import |
| [Anchors](anchors.md) | ← Where Kyte gets interesting |
| [Memory](memory.md) | Vault, auto-free, when to use the heap |

---

## Hello, World

```kyte
@main(main) {
    print("Hello, World!");
}
```

Yep. That's it.

---

## Install

```sh
# Coming soon — check the GitHub releases page
kyte --version
```
