# Anchors

Anchors are Kyte's signature feature. They're event-driven entry points that can **automatically restart on failure**. Think of them as self-healing functions — if something goes wrong inside, the anchor picks itself back up and tries again.

---

## Your First Anchor

Every Kyte program starts with a `@main(main)` anchor:

```kyte
@main(main) {
    print("Hello, Kyte!");
}
```

The `@` prefix marks it as an anchor. `main` is the kind.

---

## Anchor Kinds

| Syntax | Kind | Description |
|---|---|---|
| `@name(main)` | main | Primary program entry point |
| `@name(plain)` | plain | Simple handler, no threading |
| `@name(thread)` | thread | Runs on a separate thread |
| `@name(event(error))` | event | Triggered by an event type |

```kyte
@main(main) {
    print("entry point");
}

@worker(thread) {
    // runs in its own thread
}

@error_handler(event(error)) {
    // handles error events
}
```

---

## Kill — Triggering a Restart

`Kill` signals the anchor to restart from the top:

```kyte
@main(main) {
    int attempt = 0;
    attempt += 1;

    print(attempt);

    if attempt < 3 {
        Kill "simulating failure";   // restart!
    }

    print("stable after 3 attempts");
}
```

Output:
```
1
2
3
stable after 3 attempts
```

Wait — if the anchor restarts, why does `attempt` remember its value?

---

## Vault — Persistent State Across Restarts

Regular variables reset to their initial value on every restart. `Vault` variables survive restarts because they live on the heap:

```kyte
@main(main) {
    Vault int attempt = 0;   // heap-allocated, survives Kill
    attempt += 1;

    print(attempt);

    if attempt < 3 {
        Kill "retry";
    }

    print("done!");
}
```

Output:
```
1
2
3
done!
```

This is the classic retry pattern. Stack variables reset; Vault variables remember.

---

## Nested Anchors

Anchors can be nested inside other anchors:

```kyte
@main(main) {
    Vault int outer = 0;
    outer += 1;

    @retry(plain) {
        Vault int inner = 0;
        inner += 1;
        if inner < 2 { Kill "inner retry"; }
        print(f"inner done at {inner}");
    }

    if outer < 2 { Kill "outer retry"; }
    print(f"outer done at {outer}");
}
```

Each anchor has its own restart scope. Nested anchors don't cause the parent to restart.

---

## yield — Returning from an Anchor

Anchors can return values using `yield`:

```kyte
@compute(thread) {
    int result = heavy_calculation();
    yield result;
}
```

---

## When to Use Anchors

Anchors are great for:

- **Retry logic** — connection failures, transient errors
- **State machines** — each restart advances the state
- **Event handlers** — react to system events without polling
- **Long-running workers** — keep going even if one iteration fails

Traditional `try/catch` handles errors at the call site. Anchors handle them at the structure level — the whole anchor restarts, which is often exactly what you want.

---

## Tips

- `Kill` with no message: `Kill;` — message is optional.
- An anchor that never hits `Kill` just runs once, like a normal function.
- Infinite restart loops are possible if you never reach a stable state — add a Vault counter as a safeguard.
