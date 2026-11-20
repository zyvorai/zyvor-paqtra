# REST API

Moved from the README.

The REST API serves JSON over HTTP with JWT authentication and is documented in [docs/openapi.yaml](openapi.yaml). It includes eBPF endpoints that read kernel data through `bpftool`.

```bash
# Health check
curl http://localhost:9191/health

# List flows
curl http://localhost:9191/api/v1/flows

# List policies
curl http://localhost:9191/api/v1/policies

# Generate policy from traffic
curl -X POST http://localhost:9191/api/v1/modules/autopolicy/generate \
  -H 'Content-Type: application/json' \
  -d '{"namespace":"default","observation_duration":"5m"}'
```

Full endpoint list: [docs/web-architecture.md](web-architecture.md) and [docs/client/api-reference.html](client/api-reference.html).

| Interface | URL |
|-----------|-----|
| Web dashboard | `http://<host>:9191` |
| REST API | `http://<host>:9191/api/v1/` |
| WebSocket | `ws://<host>:9191/api/v1/ws/metrics` |
| Live node metrics | `ws://<host>:9191/api/v1/ws/metrics/live` (see [metrics.md](metrics.md)) |
| Health check | `http://<host>:9191/health` |
