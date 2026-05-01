# Modules

As code grows, a single file becomes unwieldy. Kyte offers two tools to organize it: **inline `mod` blocks** and **`import`**.

---

## Inline `mod` — Namespace Grouping

Bundle related functions under a name within one file:

```kyte
mod math {
    fn add(int a, int b) -> int {
        return a + b;
    }

    fn square(int n) -> int {
        return n * n;
    }

    fn abs(int n) -> int {
        if n < 0 { return -n; }
        return n;
    }
}

@main(main) {
    print(math.add(3, 7));    // 10
    print(math.square(5));    // 25
    print(math.abs(-12));     // 12
}
```

Call functions as `module.function()`. The namespace prevents name collisions, and typing `math.` triggers LSP autocomplete for everything inside.

`mod` holds **functions only** — no variables, no struct definitions. Those go at the top level.

---

## `import` — File Splitting

When a file gets too long, split it:

```kyte
// utils.ky
fn clamp(int val, int lo, int hi) -> int {
    if val < lo { return lo; }
    if val > hi { return hi; }
    return val;
}

fn sign(int n) -> int {
    if n > 0 { return 1; }
    if n < 0 { return -1; }
    return 0;
}
```

```kyte
// main.ky
import "utils.ky";

@main(main) {
    int x = clamp(150, 0, 100);   // 100
    int s = sign(-42);             // -1
    print(x);
    print(s);
}
```

All top-level declarations from the imported file — functions, structs, enums, consts — become available directly in the importing file. No prefix needed.

Import paths are relative to the current file's directory.

---

## Using Both Together

```kyte
import "math_utils.ky";

mod fmt {
    fn pad_left(string s, int width) -> string {
        // ...
    }
}

@main(main) {
    int result = some_math_fn(5);       // from import
    string r = fmt.pad_left("hi", 10); // from mod
}
```

---

## When to Use What

| Situation | Use |
|---|---|
| Grouping functions logically within one file | `mod` block |
| File has grown too long | `import` |
| Reusing shared library code | `import` |

---

## Tips

- Circular imports are not supported — `a.ky` importing `b.ky` which imports `a.ky` will error.
- Importing the same file from multiple places is safe — it's processed once.
- The LSP autocompletes `mod` members — type `math.` and see the list.
