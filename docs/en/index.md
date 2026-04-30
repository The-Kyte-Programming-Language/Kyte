# Kyte

> A fast, expressive systems language that doesn't get out of your way.

Kyte is a statically-typed compiled language that compiles to native code via LLVM. It's designed for developers who want Rust-level performance without the borrow-checker wrestling matches — and who want their programs to *keep running* even when things go wrong, thanks to the **Anchor** system.

---

## Quick Look

```kyte
// Good ol' struct + loop
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

Output:
```
0
1
2
3
4
```

---

## Why Kyte?

| Feature | What it means for you |
|---|---|
| **Anchors** | Event-driven entry points that restart automatically on failure. No more crash-and-burn. |
| **Vault** | Opt-in heap allocation — you decide what lives on the stack vs heap. |
| **LLVM backend** | Compiles to native machine code. Fast. |
| **LSP support** | First-class IDE experience out of the box. |
| **Simple syntax** | If you've written C, Rust, or Go, you'll feel at home in 10 minutes. |

---

## Docs Overview

| Section | What's inside |
|---|---|
| [Types & Variables](types.md) | int, float, string, bool, auto, Vault, and more |
| [Functions](functions.md) | fn, closures, generics |
| [Control Flow](control-flow.md) | if/else, for, while, loop, match |
| [Structs & Enums](structs-enums.md) | Custom types with payloads |
| [Traits & Impl](traits-impl.md) | Polymorphism, the Kyte way |
| [Modules](modules.md) | mod blocks and import |
| [Anchors](anchors.md) | The signature feature — resilient entry points |
| [Memory](memory.md) | Vault, free, and how allocation works |

---

## Install

```sh
# (coming soon — check the GitHub releases page)
kyte --version
```

---

## Hello, World

```kyte
@main(main) {
    print("Hello, World!");
}
```

Yep. That's it.
