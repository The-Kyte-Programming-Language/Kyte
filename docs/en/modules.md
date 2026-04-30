# Modules

Modules let you group related functions into a named namespace. Two ways to use them: inline `mod` blocks and `import` statements.

---

## Inline Modules

Define a module with `mod` and call its functions with `ModuleName.functionName()`:

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
    int sum = math.add(3, 7);     // 10
    int sq  = math.square(5);     // 25
    int a   = math.abs(-12);      // 12
    print(sum);
}
```

Modules can contain any number of functions. They're purely a namespace — no state, no instances.

---

## Importing Files

Split your code across multiple `.ky` files using `import`:

```kyte
// helpers.ky
fn clamp(int val, int lo, int hi) -> int {
    if val < lo { return lo; }
    if val > hi { return hi; }
    return val;
}
```

```kyte
// main.ky
import "helpers.ky";

@main(main) {
    int x = clamp(150, 0, 100);   // 100
    print(x);
}
```

Imports are resolved relative to the current file's directory. The imported file's top-level declarations become available directly — no prefix needed.

---

## Combining Both

```kyte
import "math_utils.ky";

mod string_utils {
    fn repeat(string s, int n) -> string {
        // ...
    }
}

@main(main) {
    int result = math_utils_fn(5);           // from import
    string r = string_utils.repeat("hi", 3); // from mod
}
```

---

## Tips

- `mod` is for logical grouping within a file. `import` is for splitting across files.
- Circular imports are not supported.
- The LSP provides auto-completion for module function calls — just type `math.` and see the list.
