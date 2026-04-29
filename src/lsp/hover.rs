use std::panic::{catch_unwind, AssertUnwindSafe};

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

use super::imports::preprocess_source;
use super::util::{extract_doc_comment, format_with_doc, ty_str};
use crate::ast::TopLevel;
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn compute_hover(text: &str, pos: Position) -> Option<Hover> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(pos.line as usize)?;
    let chars: Vec<char> = line.chars().collect();
    let col = pos.character as usize;
    if col >= chars.len() || !(chars[col].is_alphanumeric() || chars[col] == '_') {
        return None;
    }

    let mut lo = col;
    while lo > 0 && (chars[lo - 1].is_alphanumeric() || chars[lo - 1] == '_') {
        lo -= 1;
    }
    let mut hi = col;
    while hi < chars.len() && (chars[hi].is_alphanumeric() || chars[hi] == '_') {
        hi += 1;
    }
    let word: String = chars[lo..hi].iter().collect();

    let md = keyword_hover(&word).or_else(|| symbol_hover(text, &word))?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: Some(Range {
            start: Position {
                line: pos.line,
                character: lo as u32,
            },
            end: Position {
                line: pos.line,
                character: hi as u32,
            },
        }),
    })
}

fn keyword_hover(w: &str) -> Option<String> {
    let s = match w {
        "enum" => {
            "\
**enum** — enum type declaration\n\n\
```kyte\n\
enum Color {\n\
    Red,\n\
    Green,\n\
    Blue,\n\
}\n\
```"
        }
        "match" => {
            "\
**match** — pattern matching\n\n\
```kyte\n\
match color {\n\
    Color.Red => { print(\"red\"); }\n\
    Color.Green => { print(\"green\"); }\n\
    _ => { print(\"other\"); }\n\
}\n\
```"
        }
        "int" => {
            "\
**int** — 64-bit signed integer type\n\n\
```kyte\n\
int x = 42;\n\
int y = x + 10;\n\
```"
        }
        "float" => {
            "\
**float** — 64-bit floating-point type\n\n\
```kyte\n\
float pi = 3.14;\n\
float r = pi * 2.0;\n\
```"
        }
        "string" => {
            "\
**string** — UTF-8 string type\n\n\
```kyte\n\
string name = \"world\";\n\
print(\"Hello, \" + name);\n\
```"
        }
        "bool" => {
            "\
**bool** — boolean type\n\n\
```kyte\n\
bool flag = true;\n\
if flag { print(1); }\n\
```"
        }
        "fn" => {
            "\
**fn** — declare a function\n\n\
```kyte\n\
fn add(int a, int b) -> int {\n\
    return a + b;\n\
}\n\
```"
        }
        "struct" => {
            "\
**struct** — user-defined data type\n\n\
```kyte\n\
struct User {\n\
    string name;\n\
    int age;\n\
}\n\
```"
        }
        "Vault" => {
            "\
**Vault** — managed-memory declaration (heap-allocated)\n\n\
Vault variables are automatically freed at scope exit.\n\n\
```kyte\n\
Vault int buffer = 1024;\n\
// ... use buffer ...\n\
// automatically freed when scope ends\n\
```"
        }
        "yield" => {
            "\
**yield** — transfer data out of an anchor\n\n\
```kyte\n\
@producer() {\n\
    yield 42;\n\
}\n\
```"
        }
        "print" => {
            "\
**print(...)** — print values to stdout\n\n\
```kyte\n\
print(42);\n\
print(\"hello\");\n\
print(x + y);\n\
```"
        }
        "Kill" => {
            "\
**Kill** — terminate the current anchor with recovery\n\n\
```kyte\n\
@handler() {\n\
    Kill \"error occurred\";\n\
}\n\
```"
        }
        "Exit" => {
            "\
**Exit** — exit the entire program\n\n\
```kyte\n\
if error {\n\
    Exit;\n\
}\n\
```"
        }
        "return" => {
            "\
**return** — return a value from a function\n\n\
```kyte\n\
fn double(int n) -> int {\n\
    return n * 2;\n\
}\n\
```"
        }
        "if" => {
            "\
**if** — conditional branch\n\n\
```kyte\n\
if x > 10 {\n\
    print(\"big\");\n\
} else {\n\
    print(\"small\");\n\
}\n\
```"
        }
        "else" => {
            "\
**else** — alternative branch\n\n\
```kyte\n\
if x > 0 {\n\
    print(\"positive\");\n\
} else {\n\
    print(\"non-positive\");\n\
}\n\
```"
        }
        "loop" => {
            "\
**loop** — infinite loop (use `break` to exit)\n\n\
```kyte\n\
int i = 0;\n\
loop {\n\
    if i >= 10 { break; }\n\
    i += 1;\n\
}\n\
```"
        }
        "while" => {
            "\
**while** — conditional loop\n\n\
```kyte\n\
int i = 0;\n\
while i < 10 {\n\
    print(i);\n\
    i += 1;\n\
}\n\
```"
        }
        "for" => {
            "\
**for** — range-based loop\n\n\
```kyte\n\
for i in 0..5 {\n\
    print(i);  // 0, 1, 2, 3, 4\n\
}\n\
```"
        }
        "break" => {
            "\
**break** — exit the innermost loop\n\n\
```kyte\n\
loop {\n\
    if done { break; }\n\
}\n\
```"
        }
        "true" => {
            "\
**true** — boolean literal\n\n\
```kyte\n\
bool active = true;\n\
```"
        }
        "false" => {
            "\
**false** — boolean literal\n\n\
```kyte\n\
bool done = false;\n\
```"
        }
        "as" => {
            "\
**as** — type casting\n\n\
```kyte\n\
int x = 42;\n\
float y = x as float;\n\
```"
        }
        "import" => {
            "\
    **import** — include another Kyte source file\n\n\
    ```kyte\n\
    import \"util.ky\";\n\
    @main(main) {\n\
        print(add(1, 2));\n\
    }\n\
    ```"
        }
        "free" => {
            "\
~~**free(name)**~~ — **deprecated** (E033)\n\n\
Manual `free()` is no longer allowed.\n\
Vault variables are automatically freed at scope exit.\n\n\
```kyte\n\
Vault int buf = 512;\n\
// buf is automatically freed when scope ends\n\
```"
        }
        "auto" => {
            "\
**auto** — infer the type from the initializer\n\n\
```kyte\n\
auto x = 42;       // int\n\
auto name = \"hi\"; // string\n\
auto flag = true;  // bool\n\
```"
        }
        _ => return None,
    };
    Some(s.into())
}

fn symbol_hover(text: &str, word: &str) -> Option<String> {
    let src = preprocess_source(text);
    let r = catch_unwind(AssertUnwindSafe(|| -> Option<String> {
        let mut lex = Lexer::new(&src);
        let tokens = lex.tokenize();
        let mut par = Parser::new(tokens);
        let ast = par.parse();

        for (item, span) in &ast.items {
            match item {
                TopLevel::Function {
                    name,
                    params,
                    return_ty,
                    ..
                } if name == word => {
                    let ps: Vec<String> = params
                        .iter()
                        .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                        .collect();
                    let ret = return_ty
                        .as_ref()
                        .map(|t| format!(" -> {}", ty_str(t)))
                        .unwrap_or_default();

                    let doc = extract_doc_comment(&src, span.line);
                    let sig = format!("```kyte\nfn {}({}){}\n```", name, ps.join(", "), ret);
                    let hover_md = format_with_doc(&sig, &doc);
                    return Some(hover_md);
                }
                TopLevel::Anchor { name, kind, .. } if name == word => {
                    let doc = extract_doc_comment(&src, span.line);
                    let sig = format!("```kyte\n@{}({:?})\n```", name, kind);
                    return Some(format_with_doc(&sig, &doc));
                }
                // 중첩 앵커 탐색
                TopLevel::Anchor { children, .. } => {
                    if let Some(result) = search_children(&src, children, word) {
                        return Some(result);
                    }
                }
                _ => {}
            }
        }
        None
    }));
    r.ok().flatten()
}

fn search_children(
    src: &str,
    children: &[(TopLevel, crate::ast::Span)],
    word: &str,
) -> Option<String> {
    for (item, span) in children {
        if let TopLevel::Anchor {
            name,
            kind,
            children: nested,
            ..
        } = item
        {
            if name == word {
                let doc = extract_doc_comment(src, span.line);
                let sig = format!("```kyte\n@{}({:?})\n```", name, kind);
                return Some(format_with_doc(&sig, &doc));
            }
            if let Some(result) = search_children(src, nested, word) {
                return Some(result);
            }
        }
    }
    None
}
