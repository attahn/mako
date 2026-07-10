# Path deps + workspace sketch

Root `mako.toml` declares `[workspace] members`. Path deps still work inside members
(`app` → `helper` → `core`).

```bash
mako check examples/pkg_path_dep
mako check examples/pkg_path_dep -p app
mako fmt examples/pkg_path_dep
mako lint examples/pkg_path_dep -p app
mako build examples/pkg_path_dep -p app
mako test examples/pkg_path_dep -p app
mako run examples/pkg_path_dep -p app          # or omit -p when only one runnable member
mako run examples/pkg_path_dep/app/main.mko
```

Local-only — no SemVer registry.
