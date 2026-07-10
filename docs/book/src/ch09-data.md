# 9. Data: JSON, SQL, files

## Files

```mko
let body = read_file("config.txt")
let n = write_file("out.txt", body)
```

Path helpers via builtins or `import "path"` / `"filepath"`:

```mko
import "path"
import "filepath"

fn main() {
    let p = path.join("a", "b")
    print(path.clean("/a/../b"))
}
```

Buffered I/O: `bufio`. Walk: `filepath_walk` / `filepath_walk_n`.

## JSON

Derive and helpers cover common encode/decode paths — see GUIDE §13 and
`#[derive(json)]` examples in the tree. Prefer explicit `Result` at boundaries.

## SQL

Parameterized queries only for user data:

```mko
let n = sqlite_query_int(":memory:", "select 1")
// Prefer *_params variants with bound arguments for dynamic input
```

Unified SQL helpers and engine demos: `examples/db_engine/`,
`examples/testing/sql_unify_test.mko`, `sql_pool_test.mko`,
`sql_tx_stmt_test.mko`, `sql_migration_test.mko`, `sql_typed_check_test.mko`,
`mysql_redis_polish_test.mko`, `multistore_compat_test.mko`,
`derive_json_codegen_test.mko`, `db_params_test.mko`. Postgres/Redis clients
exist; live CI examples under `examples/ci/` need services.

## Encoding family

| Need | Reach for |
|------|-----------|
| Base64 / hex | encoding helpers |
| Gob / binary LE+BE | `gob_*`, `binary_*` |
| CSV / XML escape | `csv_*`, `xml_*` |
| Gzip / tar / zip | compress + archive packages |

## Idioms

1. Validate paths (`str_contains(path, "..")` → error) before FS work.
2. Use arenas for temporary decode buffers.
3. Never concatenate SQL — use params APIs.
4. Wipe secrets with `secret_drop` after use.

Next: [Packages & tooling workflow](ch10-packages.md).
