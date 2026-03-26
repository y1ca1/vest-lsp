# Vest for VS Code

This extension adds Vest language support to Visual Studio Code:

- `.vest` language registration
- syntax highlighting
- comment and bracket configuration
- the Vest language server (`vest_lsp`) for diagnostics, hover, go-to-definition, references, rename, and semantic tokens

## Packaging

From the repository root:

```sh
node extensions/vscode/scripts/package-vsix.mjs
```

That builds `vest_lsp`, bundles it into a platform-specific VSIX, and writes the result to `extensions/vscode/dist/`.

## Installation

```sh
code --install-extension extensions/vscode/dist/vest-0.1.0-darwin-arm64.vsix --force
```

Then open any `.vest` file in VS Code.

## Configuration

`vest.languageServer.path`
: Override the language-server executable path.

`vest.languageServer.arguments`
: Extra arguments passed to the language server.

`vest.languageServer.environment`
: Extra environment variables passed to the language server.
