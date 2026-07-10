# 13. Tooling

## Version

```bash
mako version              # mako version mako0.1.0 darwin/arm64
mako --version / -V       # same
mako version -v           # optional commit (MAKO_GIT_HASH / build.rs git)
```

## Core commands

```bash
mako check path.mko
mako build path.mko -o bin
mako run path.mko [-- args...]
mako test [path] [-r PAT] [-v] [--count N] [--coverage]
mako fmt [paths...] [-w|-l|-d]
mako lint [path]
mako bench [path]
mako profile main.mko --json
mako doc [path]              # API markdown + examples.md + search-index.json
mako init [--backend|--workspace]
mako pkg init|list|fetch|lock|add|remove|audit
mako deploy docker          # multi-stage Dockerfile + .dockerignore
mako deploy serverless      # Cloud Run / Fly.io starter manifests
mako deploy wasm            # browser/edge WASI preview1 starter
mako deploy plugin          # native/WASM plugin ABI skeletons
```

Useful flags: `--time`, `-j` / `MAKO_JOBS`, `--no-incremental`,
`--target <triple>`, `--sanitize=…`, `--static-link`, `--emit-c`.

## LSP

`mako lsp` is the language-server entry (depth still growing — STATUS/VISION).
Point your editor at the installed `mako` binary.

## Debug

| Tool | Use |
|------|-----|
| `dbg` | Lightweight debug prints in `.mko` |
| lldb / gdb | Native symbols (`-g` debug builds) |
| VS Code `mako-native` | Build active `.mko`, then launch via CodeLLDB/cpptools |
| sanitizers | `--sanitize=address\|thread` |

Walkthrough: [DEBUG.md](../../DEBUG.md) · how-to:
[03-errors-debugging](../../howto/03-errors-debugging.md).

## Docs & release

| Doc | Role |
|-----|------|
| [RELEASE.md](../../RELEASE.md) | Packaging checklist |
| [Formula/mako.rb](../../../Formula/mako.rb) | Homebrew sketch |
| [CHANGELOG.md](../../../CHANGELOG.md) | What shipped |

Next: [Cookbook](ch14-cookbook.md).
