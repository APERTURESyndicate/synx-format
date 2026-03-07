# SYNX Docker Stack Example

This example shows a practical chain:

1. `config/app.synx` is the single source of truth.
2. `scripts/render-nginx.js` parses SYNX with `strict: true` and environment overrides.
3. Nginx config is generated into `generated/default.conf`.
4. Docker Compose starts `web`, `redis`, and `nginx` using that generated config.

## Run

```bash
cd examples/docker-stack
docker compose up --abort-on-container-exit config-builder
docker compose up
```

Then open `http://localhost:8080`.

## Why this pattern

- `:env:default` keeps local/dev/prod values in one SYNX file.
- `strict: true` fails fast if `:include`, `:watch`, `:calc`, or constraints break.
- Nginx and app services stay unchanged; only config generation changes.
