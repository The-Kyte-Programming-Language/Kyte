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
| `@name(thread)` | thread | Runs on a separate OS thread |
| `@name(event(error))` | event | Triggered by `emit("error", ...)` |

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

## catch — Intercepting Kill

By default, `Kill` restarts the anchor up to 3 times, then escalates to the parent. With a `catch` block, you intercept the `Kill` before it causes a restart:

```kyte
@main(main) {
    Vault int attempt = 0;
    attempt += 1;

    if attempt < 3 {
        Kill "not ready yet";
    }

    print("all good");
} catch (string reason) {
    print(reason);
    // no break → anchor restarts
}
```

- **No `break` in catch** → anchor restarts from the top
- **`break` in catch** → anchor exits immediately

```kyte
@main(main) {
    risky_call();
    Kill "abort";
} catch (string why) {
    print(why);
    break;   // stop here — don't restart
}
```

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

## Thread Anchors

`@name(thread)` spawns a supervised OS thread. The body runs concurrently with the parent anchor:

```kyte
@main(main) {
    @worker(thread) {
        // runs on a separate thread
        Vault int count = 0;
        count += 1;
        print(count);
        if count < 5 {
            Kill "retry worker";
        }
    }

    print("parent continues here");
}
```

Thread anchors have their own restart loop — if the thread body hits `Kill`, the thread restarts independently without affecting the parent.

---

## Event Anchors

`@name(event(type))` registers a handler for a named event. The handler runs when `emit("type", payload)` is called anywhere in the program:

```kyte
@main(main) {
    @on_error(event(error)) {
        print(_payload);   // implicit string variable from emit()
    }

    emit("error", "connection lost");   // triggers on_error
    emit("error", "timeout");           // triggers on_error again
}
```

### emit()

```
emit("event_name")                  // fire event, no payload
emit("event_name", "payload string")  // fire event with payload
```

`emit()` is **synchronous** — it calls all matching handlers and waits for them to return before continuing.

### _payload

Inside an event anchor body, `_payload` is an implicit `string` variable containing the payload passed to `emit()`. If no payload was given, `_payload` is an empty string.

```kyte
@main(main) {
    @on_alert(event(alert)) {
        print(f"Alert received: {_payload}");
    }

    emit("alert", "disk almost full");
    // prints: Alert received: disk almost full
}
```

### Event anchors and Kill

Event handlers can also `Kill` to restart themselves:

```kyte
@main(main) {
    @on_request(event(request)) {
        Vault int tries = 0;
        tries += 1;
        if tries < 3 {
            Kill "retrying request";
        }
        print(f"handled: {_payload} after {tries} tries");
    }

    emit("request", "fetch /api/data");
}
```

### Multiple handlers for the same event

You can register multiple handlers for the same event name — all of them run in registration order:

```kyte
@main(main) {
    @logger(event(error)) {
        print(f"[log] {_payload}");
    }
    @alerter(event(error)) {
        print(f"[alert] {_payload}");
    }

    emit("error", "disk full");
    // prints:
    // [log] disk full
    // [alert] disk full
}
```

---

## yield — Returning from an Anchor

Anchors can return values using `yield`:

```kyte
@compute(plain) {
    int result = heavy_calculation();
    yield result;
}
```

---

## When to Use Anchors

| Kind | Use when |
|---|---|
| `plain` | Isolated retry scope inside a parent anchor |
| `thread` | CPU-bound or blocking work that should run concurrently |
| `event` | Decoupled handler triggered by a named signal |
| `main` | Program entry point (always required) |

Traditional `try/catch` handles errors at the call site. Anchors handle them at the structure level — the whole anchor restarts, which is often exactly what you want.

---

## Tips

- `Kill` with no message: `Kill;` — message is optional.
- An anchor that never hits `Kill` just runs once, like a normal function.
- Thread anchors run concurrently — use `Vault` for shared state carefully.
- Event handlers fire synchronously at the point `emit()` is called.
- `_payload` is always a `string` inside event handlers; cast with `as` if needed.
