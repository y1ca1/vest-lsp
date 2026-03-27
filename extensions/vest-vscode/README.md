# Vest for VS Code

This extension adds Vest language support to Visual Studio Code:

- `.vest` language registration
- syntax highlighting
- comment and bracket configuration
- the Vest language server (`vest_lsp`) for diagnostics, hover, go-to-definition, references, rename, and semantic tokens

## Packaging

From the repository root:

```sh
node extensions/vest-vscode/scripts/package-vsix.mjs
```

That builds `vest_lsp`, bundles it into a platform-specific VSIX, and writes the result to `extensions/vest-vscode/dist/`.

For release publishing, the workflow builds a universal VSIX instead. Published installs download the matching `vest_lsp` binary from the corresponding GitHub release for this extension version.

## Installation

### Published Extension

1. Open the Extensions view in VS Code.
2. Search for `Vest`.
3. Click Install.

### Local VSIX Install

1. Build the VSIX:

```sh
node extensions/vest-vscode/scripts/package-vsix.mjs
```

2. In VS Code, run `Extensions: Install from VSIX...`.
3. Select the generated file in `extensions/vest-vscode/dist/`.

CLI alternative:

```sh
code --install-extension extensions/vest-vscode/dist/vest-<version>-<platform>.vsix --force
```

## Configuration

`vest.languageServer.path`
: Override the language-server executable path.

`vest.languageServer.arguments`
: Extra arguments passed to the language server.

`vest.languageServer.environment`
: Extra environment variables passed to the language server.
