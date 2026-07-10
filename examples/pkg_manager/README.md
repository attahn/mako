# Package manager demo (local registry)

```bash
# From repo root, with mako on PATH:
mako pkg publish examples/pkg_manager/util
# Depend from app (registry already seeded under app/.mako/registry for offline demo):
cd examples/pkg_manager/app
mako pkg install
mako pkg audit
mako run main.mko   # → 5
```

`mako.lock` is written by `install` / `lock` / `update` for reproducible builds.
`mako pkg audit` is fully offline: it checks `mako.lock` against local
`mako-cve.toml` advisories and `mako-license.toml` license policy.
