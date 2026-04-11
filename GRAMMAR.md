# Kyte Language Grammar Specification

> **Version:** 0.1.0  
> **File Extension:** `.ky`  
> **Backend:** LLVM

---

## 1. Program Structure

Every Kyte source file must have a top-level `@main` anchor.

```
Program      ::= TopLevel*
TopLevel     ::= Anchor | Function
```

---

## 2. Anchors (`@`)

Anchors are Kyte's core control-flow and resilience primitive. They act as recovery points — when a runtime error occurs or `Kill` is invoked, resources are reclaimed and execution jumps to the nearest enclosing anchor.

```
Anchor       ::= '@' IDENT '(' AnchorKind (',' AnchorOpt)* ')'
                  AnchorBody

AnchorKind   ::= 'main'
               | 'event' '(' STRING ')'
               | 'thread'
               | 'on_error'
               | 'timeout' '(' INT ')'

AnchorOpt    ::= 'retry' '(' INT ')'

AnchorBody   ::= Stmt*                          // no braces required
```

### Rules

- Anchors can be nested. Inner anchors are children of the enclosing anchor.
- Anchors are isolated — a parent cannot access a child's Vault, and siblings cannot see each other's Vault.
- A child anchor **can** read its parent's Vault.
- `@` prefixed anchors are language-reserved. Users define decorators with `#`.

### Inline Anchors

Anchors may appear inside any block (e.g. inside `if`, `loop`). These are called **inline anchors** and follow the same syntax.

---

## 3. Functions

```
Function     ::= 'function' IDENT '(' ParamList ')' ('->' Type)? '{' Stmt* '}'
ParamList    ::= (Param (',' Param)*)?
Param        ::= Type IDENT
```

---

## 4. Types

```
Type         ::= 'int' | 'float' | 'string' | 'bool'
```

---

## 5. Statements

```
Stmt         ::= VarDecl
               | VaultDecl
               | Assign
               | CompoundAssign
               | IfStmt
               | ForStmt
               | LoopStmt
               | Kill
               | Exit
               | Yield
               | Return
               | Break
               | Free
               | InlineAnchor
               | ExprStmt
```

### 5.1 Variable Declaration

```
VarDecl      ::= Type IDENT '=' Expr ';'
```

### 5.2 Vault Declaration

Vault variables are scoped to the enclosing anchor's lifetime. They are freed when the anchor is destroyed. **Vault allocations inside loops without explicit `free` are rejected at compile time.**

```
VaultDecl    ::= 'vault' Type IDENT '=' Expr ';'
```

### 5.3 Assignment

```
Assign       ::= IDENT '=' Expr ';'
```

### 5.4 Compound Assignment

```
CompoundAssign ::= IDENT ('+=' | '-=' | '*=' | '/=' | '%=') Expr ';'
```

### 5.5 If / Else / Else If

```
IfStmt       ::= 'if' Expr '{' Stmt* '}' ElseClause?
ElseClause   ::= 'else' '{' Stmt* '}'
               | 'else' IfStmt
```

### 5.6 For Loop

```
ForStmt      ::= 'for' IDENT 'in' Expr '..' Expr '{' Stmt* '}'
```

### 5.7 Loop (infinite)

```
LoopStmt     ::= 'loop' '{' Stmt* '}'
```

### 5.8 Kill

Reclaims all resources in the current anchor and jumps to the nearest enclosing anchor. If the same anchor triggers multiple errors, execution escalates to the parent anchor.

```
Kill         ::= 'Kill' Expr? ';'
```

### 5.9 Exit

Frees all memory and terminates the program.

```
Exit         ::= 'exit' ';'
```

### 5.10 Yield

Destroys the current anchor and passes data up to the parent anchor.

```
Yield        ::= 'yield' Expr ';'
```

### 5.11 Return

```
Return       ::= 'return' Expr? ';'
```

### 5.12 Break

```
Break        ::= 'break' ';'
```

### 5.13 Free

Manual memory deallocation. If omitted, the compiler injects `free` automatically.

```
Free         ::= 'free' '(' IDENT ')' ';'
```

### 5.14 Expression Statement

```
ExprStmt     ::= Expr ';'
```

---

## 6. Expressions

### Precedence (low → high)

| Level | Operators              | Associativity |
|-------|------------------------|---------------|
| 1     | `\|\|`                  | Left          |
| 2     | `&&`                   | Left          |
| 3     | `==  !=  <  >  <=  >=` | Left          |
| 4     | `+  -`                 | Left          |
| 5     | `*  /  %`              | Left          |
| 6     | `-` (negate)  `!` (not)| Right (unary) |
| 7     | Call, Literal, Group   | —             |

```
Expr           ::= Or
Or             ::= And ('||' And)*
And            ::= Comparison ('&&' Comparison)*
Comparison     ::= Additive (('==' | '!=' | '<' | '>' | '<=' | '>=') Additive)*
Additive       ::= Multiplicative (('+' | '-') Multiplicative)*
Multiplicative ::= Unary (('*' | '/' | '%') Unary)*
Unary          ::= ('-' | '!') Unary | Primary

Primary      ::= INT_LIT
               | FLOAT_LIT
               | STRING_LIT
               | 'true' | 'false'
               | IDENT
               | IDENT '(' ArgList ')'       // function call
               | '(' Expr ')'                // grouping

ArgList      ::= (Expr (',' Expr)*)?
```

---

## 7. Operators

### Arithmetic
| Operator | Description    |
|----------|----------------|
| `+`      | Addition       |
| `-`      | Subtraction / Negation (unary) |
| `*`      | Multiplication |
| `/`      | Division       |
| `%`      | Modulo         |

### Logical
| Operator | Description    |
|----------|----------------|
| `&&`     | Logical AND    |
| `\|\|`   | Logical OR     |
| `!`      | Logical NOT (unary) |

### Comparison
| Operator | Description          |
|----------|----------------------|
| `==`     | Equal                |
| `!=`     | Not equal            |
| `<`      | Less than            |
| `>`      | Greater than         |
| `<=`     | Less than or equal   |
| `>=`     | Greater than or equal|

### Compound Assignment
| Operator | Equivalent to  |
|----------|----------------|
| `+=`     | `x = x + expr` |
| `-=`     | `x = x - expr` |
| `*=`     | `x = x * expr` |
| `/=`     | `x = x / expr` |
| `%=`     | `x = x % expr` |

---

## 8. Lexical Elements

```
IDENT        ::= [a-zA-Z_][a-zA-Z0-9_]*
INT_LIT      ::= [0-9]+
FLOAT_LIT    ::= [0-9]+ '.' [0-9]+
STRING_LIT   ::= '"' (ESCAPE | [^"\\
])* '"'
ESCAPE       ::= '\\' [nrt\\"0]
COMMENT      ::= '//' [^\n]*
```

---

## 9. Memory Model

1. **Automatic** — The compiler analyzes lifetimes and injects `free` at the optimal point.
2. **Manual** — Users may call `free(x)` explicitly. If they forget, the compiler inserts it.
3. **Vault** — Memory tied to an anchor's lifetime. Destroyed when the anchor exits/dies.

---

## 10. Error Recovery (Anchoring)

- Runtime errors are detected via interrupts.
- On error: all resources in the current anchor are reclaimed, execution jumps to the nearest anchor.
- Repeated failures in the same anchor escalate to the parent anchor.
- `Kill "message"` logs the message before jumping.

---

## 11. Decorators (`#`)

User-defined compile-time code generation decorators. Placed above functions or anchors.

```
Decorator    ::= '#' IDENT ('(' ArgList ')')?
```

> `@` is reserved for language-level anchors. Users use `#` for custom decorators.

---

## 12. Reserved Keywords

```
main  function  vault  Kill  exit  yield  return
if    else      loop   for   in    break  true  false
alloc free      int    float string bool
```
