use std::panic::{catch_unwind, AssertUnwindSafe};

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

use super::imports::preprocess_source;
use super::util::{extract_doc_comment, format_with_doc, ty_str};
use crate::ast::{AnchorKind, TopLevel};
use crate::lexer::Lexer;
use crate::parser::Parser;

pub(super) fn compute_hover(text: &str, pos: Position) -> Option<Hover> {
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(pos.line as usize)?;
    let chars: Vec<char> = line.chars().collect();
    let col = pos.character as usize;

    // 커서가 단어 위에 있지 않으면 None
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

    let hover_range = Some(Range {
        start: Position { line: pos.line, character: lo as u32 },
        end: Position { line: pos.line, character: hi as u32 },
    });

    // 커서 앞에 '.'이 있으면 struct field / enum variant 호버 시도
    let is_member = lo > 0 && chars[lo - 1] == '.';
    let base_word = if is_member {
        let before_dot: String = chars[..lo - 1].iter().collect();
        let base_start = before_dot.chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .count();
        if base_start > 0 {
            Some(before_dot[before_dot.len() - base_start..].to_string())
        } else {
            None
        }
    } else {
        None
    };

    let md = keyword_hover(&word)
        .or_else(|| {
            if let Some(ref base) = base_word {
                member_hover(text, base, &word)
            } else {
                None
            }
        })
        .or_else(|| symbol_hover(text, &word, pos))?;

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: md,
        }),
        range: hover_range,
    })
}

// ────────────────────────────────────────────────────────────────────────────
//  멤버 호버: base.member 형태에서 base의 타입을 추론하고 멤버 정보 반환
// ────────────────────────────────────────────────────────────────────────────
fn member_hover(text: &str, base: &str, member: &str) -> Option<String> {
    let src = preprocess_source(text);
    catch_unwind(AssertUnwindSafe(|| -> Option<String> {
        let tokens = Lexer::new(&src).tokenize();
        let ast = Parser::new(tokens).parse();

        // base가 enum 이름인지 확인 → 해당 variant 설명
        for (item, _) in &ast.items {
            if let TopLevel::Enum { name, variants } = item {
                if name == base {
                    if let Some(v) = variants.iter().find(|v| v.name == member) {
                        let payload = v.ty.as_ref()
                            .map(|t| format!("({})", ty_str(t)))
                            .unwrap_or_default();
                        return Some(format!(
                            "```kyte\n{}.{}{}\n```\n\n*Enum variant*",
                            name, v.name, payload
                        ));
                    }
                }
            }
        }

        // base가 변수 → 타입 추론 → struct field 탐색
        let var_ty = infer_var_type(text, base)?;
        for (item, _) in &ast.items {
            if let TopLevel::Struct { name, fields } = item {
                if *name == var_ty {
                    if let Some(f) = fields.iter().find(|f| f.name == member) {
                        return Some(format!(
                            "```kyte\n{}.{}: {}\n```\n\n*Struct field*",
                            name, f.name, ty_str(&f.ty)
                        ));
                    }
                }
            }
        }
        None
    }))
    .ok()
    .flatten()
}

// ────────────────────────────────────────────────────────────────────────────
//  심볼 호버: 함수, struct, enum, 앵커, 변수 선언
// ────────────────────────────────────────────────────────────────────────────
fn symbol_hover(text: &str, word: &str, pos: Position) -> Option<String> {
    let src = preprocess_source(text);
    let r = catch_unwind(AssertUnwindSafe(|| -> Option<String> {
        let tokens = Lexer::new(&src).tokenize();
        let mut par = Parser::new(tokens);
        let ast = par.parse();

        for (item, span) in &ast.items {
            match item {
                TopLevel::Function { name, params, return_ty, .. } if name == word => {
                    let ps: Vec<String> = params.iter()
                        .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                        .collect();
                    let ret = return_ty.as_ref()
                        .map(|t| format!(" -> {}", ty_str(t)))
                        .unwrap_or_default();
                    let doc = extract_doc_comment(&src, span.line);
                    let sig = format!("```kyte\nfn {}({}){}\n```", name, ps.join(", "), ret);
                    return Some(format_with_doc(&sig, &doc));
                }

                TopLevel::Struct { name, fields } if name == word => {
                    let field_strs: Vec<String> = fields.iter()
                        .map(|f| format!("    {}: {};", f.name, ty_str(&f.ty)))
                        .collect();
                    let doc = extract_doc_comment(&src, span.line);
                    let sig = format!(
                        "```kyte\nstruct {} {{\n{}\n}}\n```",
                        name,
                        field_strs.join("\n")
                    );
                    return Some(format_with_doc(&sig, &doc));
                }

                TopLevel::Enum { name, variants } if name == word => {
                    let var_strs: Vec<String> = variants.iter().map(|v| {
                        if let Some(ref t) = v.ty {
                            format!("    {}({}),", v.name, ty_str(t))
                        } else {
                            format!("    {},", v.name)
                        }
                    }).collect();
                    let doc = extract_doc_comment(&src, span.line);
                    let sig = format!(
                        "```kyte\nenum {} {{\n{}\n}}\n```",
                        name,
                        var_strs.join("\n")
                    );
                    return Some(format_with_doc(&sig, &doc));
                }

                TopLevel::Trait { name, methods } if name == word => {
                    let method_strs: Vec<String> = methods.iter().map(|m| {
                        let ps: Vec<String> = m.params.iter()
                            .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                            .collect();
                        let ret = m.return_ty.as_ref()
                            .map(|t| format!(" -> {}", ty_str(t)))
                            .unwrap_or_default();
                        format!("    fn {}({}){};", m.name, ps.join(", "), ret)
                    }).collect();
                    let doc = extract_doc_comment(&src, span.line);
                    let sig = format!(
                        "```kyte\ntrait {} {{\n{}\n}}\n```",
                        name, method_strs.join("\n")
                    );
                    return Some(format_with_doc(&sig, &doc));
                }

                TopLevel::Anchor { name, kind, .. } if name == word => {
                    let kind_str = anchor_kind_str(kind);
                    let doc = extract_doc_comment(&src, span.line);
                    let sig = format!("```kyte\n@{}({})\n```", name, kind_str);
                    return Some(format_with_doc(&sig, &doc));
                }

                TopLevel::ConstDecl { ty, name, .. } if name == word => {
                    return Some(format!("```kyte\nconst {} {}\n```\n\n*Global constant*", ty_str(ty), name));
                }

                TopLevel::Anchor { children, .. } => {
                    if let Some(result) = search_children(&src, children, word) {
                        return Some(result);
                    }
                }

                _ => {}
            }
        }

        // 로컬 변수 선언 호버
        if let Some(md) = local_var_hover(text, word, pos) {
            return Some(md);
        }

        None
    }));
    r.ok().flatten()
}

// ────────────────────────────────────────────────────────────────────────────
//  로컬 변수 호버: 커서 위치에서 소스를 위로 스캔해 선언 타입을 찾음
// ────────────────────────────────────────────────────────────────────────────
fn local_var_hover(text: &str, name: &str, pos: Position) -> Option<String> {
    let scan_end = (pos.line as usize).min(text.lines().count());
    for line in text.lines().take(scan_end + 1) {
        let trimmed = line.trim();
        // `type name = ...` 또는 `Vault type name = ...`
        for prefix in &["int ", "float ", "string ", "bool ", "auto ",
                        "i8 ", "i16 ", "i32 ", "i64 ",
                        "u8 ", "u16 ", "u32 ", "u64 "] {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                let var: String = rest.chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if var == name {
                    let ty = prefix.trim();
                    return Some(format!(
                        "```kyte\n{} {}\n```\n\n*Local variable*",
                        ty, name
                    ));
                }
            }
        }
        // Vault 접두사 처리
        if let Some(rest) = trimmed.strip_prefix("Vault ") {
            for prefix in &["int ", "float ", "string ", "bool "] {
                if let Some(rest2) = rest.strip_prefix(prefix) {
                    let var: String = rest2.chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if var == name {
                        let ty = prefix.trim();
                        return Some(format!(
                            "```kyte\nVault {} {}\n```\n\n*Managed-memory variable (auto-freed)*",
                            ty, name
                        ));
                    }
                }
            }
        }
        // for 루프 변수
        if let Some(rest) = trimmed.strip_prefix("for ") {
            let var: String = rest.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if var == name {
                return Some(format!(
                    "```kyte\nfor {} in ...\n```\n\n*Loop variable (int)*",
                    name
                ));
            }
        }
    }
    None
}

/// 변수 선언을 스캔해 타입명 반환 (struct field 호버용)
pub(super) fn infer_var_type(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        // `StructName varname = ...`  — 식별자로 시작하고 공백+변수명
        let mut parts = trimmed.splitn(3, ' ');
        if let (Some(ty_token), Some(var_token)) = (parts.next(), parts.next()) {
            let var_trimmed: String = var_token.chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if var_trimmed == name && ty_token.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
                return Some(ty_token.to_string());
            }
        }
    }
    None
}

fn anchor_kind_str(kind: &AnchorKind) -> String {
    match kind {
        AnchorKind::Main => "main".to_string(),
        AnchorKind::Plain => "plain".to_string(),
        AnchorKind::Thread => "thread".to_string(),
        AnchorKind::Event(e) => format!("event({})", e),
    }
}

fn search_children(
    src: &str,
    children: &[(TopLevel, crate::ast::Span)],
    word: &str,
) -> Option<String> {
    for (item, span) in children {
        match item {
            TopLevel::Anchor { name, kind, children: nested, .. } => {
                if name == word {
                    let kind_str = anchor_kind_str(kind);
                    let doc = extract_doc_comment(src, span.line);
                    let sig = format!("```kyte\n@{}({})\n```", name, kind_str);
                    return Some(format_with_doc(&sig, &doc));
                }
                if let Some(result) = search_children(src, nested, word) {
                    return Some(result);
                }
            }
            TopLevel::Function { name, params, return_ty, .. } if name == word => {
                let ps: Vec<String> = params.iter()
                    .map(|p| format!("{} {}", ty_str(&p.ty), p.name))
                    .collect();
                let ret = return_ty.as_ref()
                    .map(|t| format!(" -> {}", ty_str(t)))
                    .unwrap_or_default();
                let doc = extract_doc_comment(src, span.line);
                let sig = format!("```kyte\nfn {}({}){}\n```", name, ps.join(", "), ret);
                return Some(format_with_doc(&sig, &doc));
            }
            _ => {}
        }
    }
    None
}

// ────────────────────────────────────────────────────────────────────────────
//  키워드 호버
// ────────────────────────────────────────────────────────────────────────────
fn keyword_hover(w: &str) -> Option<String> {
    let s = match w {
        "fn" => "\
**fn** — declare a function\n\n\
```kyte\n\
fn add(int a, int b) -> int {\n\
    return a + b;\n\
}\n\
```",
        "struct" => "\
**struct** — user-defined data type\n\n\
```kyte\n\
struct User {\n\
    string name;\n\
    int age;\n\
}\n\
```",
        "enum" => "\
**enum** — enum type declaration\n\n\
```kyte\n\
enum Color {\n\
    Red,\n\
    Green,\n\
    Blue,\n\
}\n\
```",
        "match" => "\
**match** — pattern matching\n\n\
```kyte\n\
match color {\n\
    Color.Red => { print(\"red\"); }\n\
    Color.Green => { print(\"green\"); }\n\
    _ => { print(\"other\"); }\n\
}\n\
```",
        "int" => "**int** — 64-bit signed integer\n\n```kyte\nint x = 42;\n```",
        "float" => "**float** — 64-bit floating-point\n\n```kyte\nfloat pi = 3.14;\n```",
        "string" => "**string** — UTF-8 string\n\n```kyte\nstring name = \"hello\";\n```",
        "bool" => "**bool** — boolean\n\n```kyte\nbool flag = true;\n```",
        "i8"  => "**i8** — 8-bit signed integer (-128 .. 127)",
        "i16" => "**i16** — 16-bit signed integer (-32768 .. 32767)",
        "i32" => "**i32** — 32-bit signed integer",
        "i64" => "**i64** — 64-bit signed integer (same as `int`)",
        "u8"  => "**u8** — 8-bit unsigned integer (0 .. 255)",
        "u16" => "**u16** — 16-bit unsigned integer",
        "u32" => "**u32** — 32-bit unsigned integer",
        "u64" => "**u64** — 64-bit unsigned integer",
        "Vault" => "\
**Vault** — managed-memory declaration (heap-allocated)\n\n\
Vault variables are automatically freed at scope exit.\n\n\
```kyte\n\
Vault int buf = 1024;\n\
// automatically freed when scope ends\n\
```",
        "yield" => "\
**yield** — transfer data out of an anchor\n\n\
```kyte\n\
@producer() {\n\
    yield 42;\n\
}\n\
```",
        "print" => "\
**print(...)** — print values to stdout\n\n\
```kyte\n\
print(42);\n\
print(\"hello\", x);\n\
```",
        "Kill" => "\
**Kill** — terminate the current anchor with optional message\n\n\
```kyte\n\
Kill \"error message\";\n\
```",
        "Exit" => "**Exit** — exit the entire program\n\n```kyte\nExit;\n```",
        "return" => "**return** — return a value from a function\n\n```kyte\nreturn value;\n```",
        "if" => "\
**if** — conditional branch\n\n\
```kyte\n\
if x > 10 {\n\
    print(\"big\");\n\
} else {\n\
    print(\"small\");\n\
}\n\
```",
        "else" => "**else** — alternative branch of `if`",
        "loop" => "\
**loop** — infinite loop (use `break` to exit)\n\n\
```kyte\n\
loop {\n\
    if done { break; }\n\
}\n\
```",
        "while" => "\
**while** — conditional loop\n\n\
```kyte\n\
while i < 10 {\n\
    i += 1;\n\
}\n\
```",
        "for" => "\
**for** — range-based loop\n\n\
```kyte\n\
for i in 0..10 {\n\
    print(i);\n\
}\n\
```",
        "break" => "**break** — exit the innermost loop (or exit an anchor from inside a catch block)",
        "catch" => "\
**catch** — handle Kill or runtime errors in an anchor\n\n\
```kyte\n\
@task(plain) {\n\
    risky_op();\n\
    Kill \"something went wrong\";\n\
} catch (string reason) {\n\
    print(reason);   // inspect the Kill message\n\
    // no break → anchor restarts automatically\n\
    // break    → anchor exits permanently\n\
    break;\n\
}\n\
```\n\
**Semantics:**\n\
- `Kill` or runtime error → always enters the catch block first\n\
- Fallthrough (no `break`) → anchor restarts from the top\n\
- `break` inside catch → anchor exits, no restart",
        "true"  => "**true** — boolean literal",
        "false" => "**false** — boolean literal",
        "as"    => "\
**as** — type casting\n\n\
```kyte\n\
float y = x as float;\n\
```",
        "import" => "\
**import** — include another Kyte source file\n\n\
```kyte\n\
import \"util.ky\";\n\
```",
        "free" => "\
~~**free(name)**~~ — **deprecated** (E033)\n\n\
Vault variables are automatically freed at scope exit.",
        "auto" => "\
**auto** — infer the type from the initializer\n\n\
```kyte\n\
auto x = 42;       // int\n\
auto name = \"hi\"; // string\n\
```",
        "assert" => "\
**assert(cond)** — runtime assertion\n\n\
```kyte\n\
assert(x > 0);\n\
assert(x > 0, \"x must be positive\");\n\
```",
        "const" => "**const** — immutable named constant\n\n```kyte\nconst int MAX = 100;\n```",
        "trait" => "\
**trait** — declare an abstract interface\n\n\
```kyte\n\
trait Printable {\n\
    fn to_string(self) -> string;\n\
}\n\
```",
        "impl" => "\
**impl** — implement a trait for a type\n\n\
```kyte\n\
impl Printable for User {\n\
    fn to_string(self) -> string {\n\
        return self.name;\n\
    }\n\
}\n\
```",
        "mod" => "\
**mod** — declare a module/namespace\n\n\
```kyte\n\
mod math {\n\
    fn abs(int x) -> int { ... }\n\
}\n\
math.abs(-5);\n\
```",
        _ => return None,
    };
    Some(s.into())
}
