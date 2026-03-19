# Vest Language Extension for Zed

This extension provides language support for the Vest DSL in the Zed editor.

## Features

- Syntax highlighting via Tree-sitter
- Code folding for definitions and combinators
- Symbol outline navigation
- Auto-indentation
- Bracket matching

## Installation

### From Source

1. Clone this repository
2. Build the tree-sitter grammar:
   ```bash
   cd vest_syntax
   tree-sitter generate
   ```
3. Link the extension to Zed's extensions directory:
   ```bash
   ln -s /path/to/vest-lsp/extensions/vest ~/.config/zed/extensions/vest
   ```
4. Restart Zed

### Language Server (Coming in Milestone 2)

The Vest LSP will provide:
- Diagnostics
- Go to definition
- Hover information
- Completions

## File Association

Files with the `.vest` extension are automatically associated with the Vest language.

## Grammar

The Vest grammar is defined in `vest_syntax/grammar.js` and supports:
- Format/combinator definitions
- Struct combinators with fields
- Enum definitions (exhaustive and non-exhaustive)
- Choice combinators (dependent and non-dependent)
- Array and Vec combinators
- Length expressions with arithmetic
- Constraints on integers and enums
- Macro definitions and invocations
- Wrap combinators with const values
