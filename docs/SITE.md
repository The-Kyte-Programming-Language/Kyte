# Docs Site Setup Guide

This directory contains the documentation source for the Kyte language.

## Structure

```
docs/
  en/           # English docs
    index.md        — Landing page + feature overview
    types.md        — Types & Variables
    functions.md    — Functions, closures, generics
    control-flow.md — if/for/while/loop/match
    structs-enums.md — Structs & Enums
    traits-impl.md  — Traits & Impl
    modules.md      — mod blocks & import
    anchors.md      — The Anchor system (unique feature)
    memory.md       — Vault & memory management
  ko/           # Korean docs (same structure)
```

## Recommended Site Generators

### Option A — VitePress (recommended)
Modern, fast, Vue-based. Great i18n support for en/ko.
```sh
npm init vitepress
```
Sidebar config: nest `en/` and `ko/` as separate locales.

### Option B — mdBook
Rust-native, fits the project. Simple config.
```sh
cargo install mdbook
mdbook init docs-site
```
Copy markdown files in and configure `SUMMARY.md`.

### Option C — Docusaurus
React-based, battle-tested i18n.
```sh
npx create-docusaurus@latest docs-site classic
```

## Page Order (sidebar)

1. Introduction
2. Types & Variables
3. Functions
4. Control Flow
5. Structs & Enums
6. Traits & Impl
7. Modules
8. Anchors  ← highlight this — it's the unique selling point
9. Memory Management
