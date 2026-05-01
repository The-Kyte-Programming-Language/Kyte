# Anchors

Anchors are Kyte's signature feature. They're not just functions — they're **structured entry points that can restart themselves on failure**.

Normal error handling forces you to choose between try/catch pyramids or letting the process die. Anchors offer a third option: restart from the top and try again, with state preserved exactly where you want it.

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

| Syntax | Description |
|---|---|
| `@name(main)` | Program entry point. Exactly one per program. |
| `@name(plain)` | Independent retry scope inside a parent anchor. |
| `@name(thread)` | Runs on a separate OS thread. |
| `@name(event(name))` | Handler triggered by `emit()`. |

---

## Kill — Triggering a Restart

`Kill` restarts the anchor from the top:

```kyte
@main(main) {
    Vault int attempts = 0;
    attempts += 1;

    print(attempts);

    if attempts < 3 {
        Kill "not ready yet";
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

`Vault int attempts` is the key. The variable lives on the heap, so it survives every restart. This is the retry pattern: **Vault for the counter, Kill to retry**.

---

## Stack vs Vault Across Restarts

The difference matters:

```kyte
@main(main) {
    int stack_val = 0;        // resets to 0 on every restart
    Vault int heap_val = 0;   // survives restarts

    stack_val += 1;
    heap_val += 1;

    print(f"stack: {stack_val}, heap: {heap_val}");

    if heap_val < 3 {
        Kill "retry";
    }
}
```

Output:
```
stack: 1, heap: 1
stack: 1, heap: 2
stack: 1, heap: 3
```

`stack_val` is always 1 — it re-initializes to 0 every restart. `heap_val` accumulates because it's on the heap.

---

## catch — Intercepting Kill

By default, `Kill` restarts the anchor. A `catch` block lets you observe or react to the Kill before the restart happens:

```kyte
@main(main) {
    Vault int attempt = 0;
    attempt += 1;

    if attempt < 3 {
        Kill "not ready yet";
    }

    print("all good");
} catch (string reason) {
    print(f"Kill fired: {reason}");
    // no break → anchor restarts
}
```

Output:
```
Kill fired: not ready yet
Kill fired: not ready yet
all good
```

**`break` inside catch** exits the anchor immediately without restarting:

```kyte
@main(main) {
    risky_call();
} catch (string why) {
    print(f"failed: {why}");
    break;   // stop here
}
```

| Catch behavior | Result |
|---|---|
| no `break` | anchor restarts |
| `break` | anchor exits |

---

## Nested Anchors

Anchors can nest inside other anchors. Inner restarts stay contained — the outer anchor is unaffected:

```kyte
@main(main) {
    Vault int outer = 0;
    outer += 1;

    @inner(plain) {
        Vault int count = 0;
        count += 1;
        if count < 2 { Kill "inner retry"; }
        print(f"inner done: {count}");
    }

    if outer < 2 { Kill "outer retry"; }
    print(f"outer done: {outer}");
}
```

Output:
```
inner done: 2
inner done: 2
outer done: 2
```

`@inner` restarts twice before succeeding. `outer` doesn't change during those restarts.

---

## Thread Anchors

`@name(thread)` runs on a separate OS thread concurrently with the parent:

```kyte
@main(main) {
    @worker(thread) {
        Vault int count = 0;
        count += 1;
        print(f"worker: {count}");
        if count < 3 { Kill "worker retry"; }
        print("worker done");
    }

    print("main continues");
}
```

If the worker hits `Kill`, only the worker restarts — the parent anchor is not affected. Use thread anchors for CPU-intensive work or blocking I/O you want off the main path.

---

## Event Anchors

`@name(event(type))` registers a named event handler. It fires whenever `emit("type", ...)` is called:

```kyte
@main(main) {
    @on_error(event(error)) {
        print(f"[ERROR] {_payload}");
    }

    emit("error", "connection lost");
    emit("error", "timeout");
    print("continuing...");
}
```

Output:
```
[ERROR] connection lost
[ERROR] timeout
continuing...
```

### emit()

```kyte
emit("event_name");                    // no payload
emit("event_name", "payload string");  // with string payload
```

`emit()` is **synchronous** — all matching handlers run to completion before the next line executes.

### _payload

Inside an event anchor, `_payload` is the string passed as the second argument to `emit()`. Empty string if no payload was given.

### Multiple handlers for the same event

All of them run in registration order:

```kyte
@main(main) {
    @logger(event(error)) {
        print(f"[log] {_payload}");
    }
    @alerter(event(error)) {
        print(f"[alert] {_payload}");
    }

    emit("error", "disk full");
}
```

Output:
```
[log] disk full
[alert] disk full
```

---

## When to Use Which Anchor

| Situation | Anchor kind |
|---|---|
| Program entry point | `main` |
| Isolate retry scope to one section | `plain` (nested) |
| CPU-bound or blocking I/O | `thread` |
| Decoupled handler for a named signal | `event` |

---

## Tips

- `Kill` with no message: `Kill;` — the message is optional.
- An anchor that never hits `Kill` runs once, like a normal function.
- Thread anchors run concurrently — be careful with shared `Vault` state.
- Event handlers fire synchronously at the `emit()` call site.
- `_payload` is always `string`; use `as` to cast if needed.
