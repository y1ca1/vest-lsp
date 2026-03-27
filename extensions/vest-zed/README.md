# Vest for Zed

This extension adds Vest language support to the Zed editor:

- `.vest` file association
- tree-sitter syntax highlighting
- comment and bracket configuration
- language server integration for diagnostics, hover, go-to-definition, references, rename, and semantic tokens

## Installation

### Published Extension

1. Open the Extensions panel in Zed.
2. Search for `Vest`.
3. Click Install.

The published extension is designed to use Zed's native installation flow. On first use it will try to download a matching `vest_lsp` binary from the `v<extension-version>` GitHub release tag for `y1ca1/vest-lsp`.

### Local Development Install

Build a clean extension directory:

```sh
node extensions/vest-zed/scripts/package-extension.mjs
```

Then in Zed:

1. Open the Extensions panel.
2. Click `Install Dev Extension`.
3. Choose `extensions/vest-zed/dist/vest-zed`.

## Language Server

The release workflow currently uploads GitHub release assets for:

- `vest_lsp-mac-aarch64.gz`
- `vest_lsp-mac-x8664.gz`
- `vest_lsp-linux-aarch64.gz`
- `vest_lsp-linux-x8664.gz`
- `vest_lsp-windows-x8664.gz`

The extension also knows how to consume `vest_lsp-windows-aarch64.gz` if you decide to add that asset in a future workflow revision.

For local development, the extension falls back to `vest_lsp` on `PATH`, and when you open a `vest-lsp` workspace checkout it can also run the server via `cargo`.

To build the language server from the repository root:

```sh
cargo build --release --package vest_lsp
```

To build a release asset for the current platform:

```sh
node extensions/vest-zed/scripts/package-language-server-asset.mjs
```

## Features

- **Syntax highlighting** via tree-sitter grammar
- **Diagnostics** for parse errors and semantic issues
- **Hover** information with wire lengths and type details
- **Go to Definition** for format references
- **Find References** for symbols
- **Rename** for identifiers
- **Semantic Tokens** for enhanced highlighting

## Configuration

The language server can be configured in your Zed settings. See the [Zed LSP documentation](https://zed.dev/docs/configuring-languages) for details.
