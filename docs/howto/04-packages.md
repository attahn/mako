# Packages

```bash
mako pkg init
mako pkg add util path=../util
mako pkg install          # SemVer resolve + mako.lock
mako pkg install --offline # require local path deps, .mako/deps, or registry cache
mako pkg list
mako pkg publish          # local registry .mako/registry or $MAKO_REGISTRY
mako pkg update
mako pkg audit            # offline advisory + license policy check
```

`mako.toml`:

```toml
name = "app"
version = "0.1.0"

[dependencies]
"util" = { path = "../util", version = "^0.1.0" }
```

Example: `examples/pkg_manager/`. Workspace: `mako init --workspace`.

`mako pkg audit` reads `mako.lock`, then checks optional local policy files:

```toml
# mako-cve.toml
[[advisory]]
id = "CVE-YYYY-NNNN"
name = "util"
version = "<=1.2.3"
severity = "high"
```

```toml
# mako-license.toml
allow = ["MIT", "Apache-2.0"]
deny = ["GPL-3.0"]

[licenses]
util = "MIT"
```

Private and offline builds use filesystem state only: path dependencies,
`.mako/deps/<name>` git caches, and `.mako/registry` or `$MAKO_REGISTRY`.
Use `mako pkg install --offline`, `mako pkg lock --offline`, or
`mako pkg update --offline` to fail fast instead of fetching missing git
dependencies. `mako pkg audit` also needs no network, so teams can vendor
policy files beside `mako.lock`.
