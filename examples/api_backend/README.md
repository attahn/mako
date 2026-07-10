# API backend example

In-memory JSON notes API on `:18200`.

```bash
mako build examples/api_backend/main.mko -o out/api_backend
./out/api_backend 20 &
curl -sS http://127.0.0.1:18200/health
```

See GUIDE.md § Building APIs.
