# Vest Language Server

A language server implementation for the [Vest DSL](https://github.com/secure-foundations/vest) — a domain-specific language for defining binary data formats featuring auto-generation of formally verified parsers and serializers in Rust/Verus.

## Features

- **Incremental Parsing** — Fast, error-tolerant parsing using tree-sitter
- **Semantic Tokens** — Enhanced syntax highlighting with tree-sitter
- **Diagnostics** — Real-time error reporting for syntax and semantic issues
- **Hover Information** — *Wire lengths*, format details, and documentation
- **Go to Definition** — Navigate to formats, enum variants, and constant definitions
- **Find References** — Locate all usages of a symbol
- **Rename** — Safely rename symbols across the file
- **Completions** — Keyword and type completions

## Installation

### Building from Source

Requires Rust 1.85 or later.

```sh
git clone https://github.com/y1ca1/vest-lsp.git
cd vest-lsp
cargo build --release
```

The binary will be at `target/release/vest_lsp`.

### VS Code

Published install:
- Open the Extensions view, search for `Vest`, and install it.

Local install:
- Build a VSIX with `node extensions/vest-vscode/scripts/package-vsix.mjs`
- In VS Code, run `Extensions: Install from VSIX...` and choose the generated file in `extensions/vest-vscode/dist/`

Published installs use a universal VSIX and download the matching `vest_lsp` binary from the corresponding GitHub release on first launch. Local VSIX builds bundle the current-platform server by default.

### Zed

Published install:
- Open the Extensions panel, search for `Vest`, and install it.

Local install:
- Build a clean extension directory with `node extensions/vest-zed/scripts/package-extension.mjs`
- In Zed, use `Install Dev Extension` and choose `extensions/vest-zed/dist/vest-zed`

The Zed extension is set up to use the native Zed installation flow. For published installs it can download a matching `vest_lsp` release asset from GitHub automatically, and for local development it still falls back to `vest_lsp` on `PATH` or `cargo run` from a `vest-lsp` workspace checkout.

### Releasing

Tagging `vX.Y.Z` triggers the release workflow in `.github/workflows/release-extensions.yml`. The workflow:

- syncs the VS Code and Zed extension versions to the workspace version
- builds the universal VS Code VSIX
- builds the Zed extension package
- builds native `vest_lsp` release assets for the supported platforms
- attaches all extension artifacts to the GitHub release
- publishes the VS Code extension automatically when the `VSCE_PAT` GitHub Actions secret is configured

## Project Structure

```
vest-lsp/
├── vest_lsp/       # Language server executable
├── vest_db/        # Salsa-based incremental database
├── vest_syntax/    # Tree-sitter grammar and parser
├── vest_corpus/    # Test corpus (good and bad examples)
├── extensions/     # Editor extensions
│   ├── vest-vscode/
│   └── vest-zed/
└── typing-rules.md # An informal DSL Type system specification
```

## Architecture

The language server is built on:

- **[Tree-sitter](https://tree-sitter.github.io/)** — Incremental parsing with error recovery
- **[Salsa](https://salsa-rs.github.io/salsa/)** — Demand-driven incremental computation
- **[async-lsp](https://docs.rs/async-lsp)** — Asynchronous LSP framework

## Development

### Running Tests

```sh
cargo test
```

### Running the Server

```sh
cargo run --release --package vest_lsp
```

The server communicates over stdio using the Language Server Protocol.

### Grammar Development

The tree-sitter grammar is in `vest_syntax/`. To regenerate the parser after grammar changes:

```sh
cd vest_syntax
tree-sitter generate
```

## License

MIT — see [LICENSE](./LICENSE) for details.

## Contributing

Contributions are welcome! Please open an issue or pull request on GitHub.
