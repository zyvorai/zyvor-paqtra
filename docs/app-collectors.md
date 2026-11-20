# Application collectors

The agent can scrape the status endpoints of common infrastructure software
alongside host, network and pod metrics (see [metrics.md](metrics.md)).
Application series go through the same pipeline: per-second store, streaming
to the API, anomaly detection, metric alerts, exporters and the console.

Collectors are **read-only clients**. They send `GET` requests or read-only
protocol commands (`INFO`, `stats`). They never write to an application,
change its configuration or run admin commands.

## Supported kinds

| Kind | Target | What is collected |
|---|---|---|
| `nginx` | `url` of `stub_status` | connections, reading/writing/waiting, accepted vs handled, requests/s |
| `apache` | `url` of `mod_status?auto` | requests/s, bytes/s, busy/idle workers, scoreboard |
| `haproxy` | `url` of the stats CSV (`;csv`) | sessions, bytes, errors, HTTP response classes, status per frontend/backend/server |
| `redis` | `address` (`host:port`) | clients, memory, commands/s, hits and misses, evictions, network, replication, keys per DB |
| `memcached` | `address` | connections, operations/s, hits and misses, memory, items, evictions |
| `envoy` | `url` of `/stats/prometheus` | liveness, memory, upstream connections, requests, retries, response classes |
| `coredns` | `url` of `/metrics` | requests, rcodes, duration (mean), cache, forwards, panics |
| `etcd` | `url` of `/metrics` | leader, proposals, DB size, WAL fsync and commit duration, peer RTT |
| `prometheus` | any Prometheus text endpoint | whatever `include` / `exclude` select |

Each instance produces contexts named `<kind>.<metric>` with charts named
`<kind>_<name>.<metric>`, labelled `app_name` plus any configured labels.
`apps.up` has one dimension per application (`kind/name`): 1 when the last
scrape succeeded and 0 when it failed. The built-in `app_down` alert watches
it.

Prometheus endpoints: counters become per-second rates, gauges stay absolute,
histograms and summaries contribute `_sum` / `_count` rates and a `_mean`.
Each label set is one dimension. At most `max_series` series (default 2,000)
per instance.

Applications are scraped every 5 seconds. A failing application is retried
with backoff (up to ten intervals).

## Static configuration

`PAQTRA_METRICS_APPS_FILE` points at a YAML file:

```yaml
apps:
  - kind: nginx
    name: edge
    url: http://10.0.3.7:8080/nginx_status
  - kind: redis
    name: cache
    address: redis.cache.svc:6379
    password_env: REDIS_CACHE_PASSWORD
  - kind: prometheus
    name: postgres
    url: http://pg-exporter.data.svc:9187/metrics
    include: ["pg_up", "pg_stat_database_*"]
    labels: {team: data}
```

| Field | Meaning |
|---|---|
| `kind` | Required, one of the kinds above. |
| `name` | Instance name; defaults to the kind. |
| `url` | HTTP(S) endpoint; required except for `redis` and `memcached`. |
| `address` | `host:port` for `redis` and `memcached`. |
| `username_env`, `password_env` | Names of environment variables holding basic-auth or Redis `AUTH` credentials. |
| `bearer_env` | Name of an environment variable holding a bearer token. |
| `include`, `exclude` | Metric-name globs for Prometheus endpoints. |
| `max_series` | Series cap per instance. |
| `labels` | Extra chart labels. |
| `insecure_skip_verify` | Skip TLS verification for this instance only. |
| `timeout` | Seconds per scrape. |

### Credentials

Credentials never appear in the file. The `*_env` fields name environment
variables the operator sets on the agent, typically from a Secret through
`agent.metrics.extraEnv`. **Paqtra never reads Kubernetes Secrets through the
API** and never takes credentials from a process's environment or command
line. Discovered applications get no credentials.

## Discovery

With `PAQTRA_METRICS_APP_DISCOVERY=true` (the default), each agent lists the
pods on its own node every 30 seconds and scrapes those annotated as:

| Annotation | Meaning |
|---|---|
| `paqtra.io/app-kind` | One of the kinds above. Wins over `prometheus.io/*`. |
| `paqtra.io/app-port`, `paqtra.io/app-path`, `paqtra.io/app-scheme` | Override the kind's default port, path and scheme (`http` / `https`). |
| `prometheus.io/scrape: "true"` | Scrape as `prometheus`; needs `prometheus.io/port`; `prometheus.io/path` (default `/metrics`) and `prometheus.io/scheme` optional. |

A discovered app is named `<namespace>_<pod>` and labelled `k8s_namespace`,
`k8s_pod` and, when the pod has an owner, `k8s_workload`. Discovery reads pod
metadata only.

## Helm

```yaml
agent:
  metrics:
    appDiscovery: true
    apps:
      - kind: redis
        name: cache
        address: redis.cache.svc:6379
        password_env: REDIS_PASSWORD
    extraEnv:
      - name: REDIS_PASSWORD
        valueFrom:
          secretKeyRef: {name: redis-auth, key: password}
```

`apps` is rendered into the `<release>-agent-apps` ConfigMap and mounted at
`/etc/paqtra/apps.yaml`.

## Databases

There is no native PostgreSQL or MySQL collector; that would need database
credentials and a driver in the agent. Run
[postgres_exporter](https://github.com/prometheus-community/postgres_exporter)
or [mysqld_exporter](https://github.com/prometheus/mysqld_exporter) and point
a `prometheus` entry (or the `prometheus.io/*` annotations) at it.
