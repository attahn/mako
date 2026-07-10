# Mako for VS Code

VS Code support for `.mko` files.

## Features

- Syntax highlighting for Mako.
- Language configuration for comments, brackets, and auto-closing pairs.
- Snippets for `main`, tests, `crew`, `arena`, and HTTP route helpers.
- Commands:
  - `Mako: Check`
  - `Mako: Build`
  - `Mako: Run`
  - `Mako: Test`
  - `Mako: Format Current File`
  - `Mako: Initialize Project`
  - `Mako: Debug Active File`
  - `Mako: Restart Language Server`
- Problem matcher for `file:line:col: error: message` diagnostics.
- Built-in `mako lsp` stdio client for diagnostics, hover, completion,
  definitions, references, rename, code actions, document/workspace symbols,
  and signature help.
- Native debug launch support through VS Code debugger adapters:
  `mako-native` builds the active file with `mako build` and delegates launch to
  CodeLLDB (`lldb`) or Microsoft C/C++ (`cppdbg`).

## Development

Open this directory in VS Code and run the extension host.

The extension expects `mako` on `PATH`. Override with:

```json
{
  "mako.path": "/path/to/mako",
  "mako.lsp.enabled": true,
  "mako.debug.adapter": "lldb"
}
```

Use **Mako: Debug Active File** or add a launch config:

```json
{
  "type": "mako-native",
  "request": "launch",
  "name": "Mako: Debug active file",
  "source": "${file}",
  "program": "${workspaceFolder}/${fileBasenameNoExtension}",
  "cwd": "${workspaceFolder}",
  "args": []
}
```

Install CodeLLDB for the default `lldb` adapter, or set
`mako.debug.adapter` / `adapter` to `cppdbg` when using Microsoft C/C++.

## Packaging

This scaffold intentionally has no runtime npm dependencies. Package it with
`vsce package` once publisher metadata is finalized.
