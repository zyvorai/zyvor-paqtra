# Per-second metrics platform

Every Paqtra agent collects host, network, filesystem, pod and application
metrics once a second, keeps them in a small local store and streams them to
`paqtra-api`. The API stores them in three rollup tiers, adds series derived
from Hubble flows, scores anomalies, evaluates metric alerts and can export
everything to Prometheus remote write, OTLP or Graphite.

The platform is **observe-only**. Collectors read `/proc`, `/sys`, cgroup files
and application status endpoints. They never write to the host, a Cilium map or
an application. Metric alerts notify; they never apply policy. Enforcement stays
with Cilium CRDs.

Related docs: [metric-alerts.md](metric-alerts.md),
[anomaly-detection.md](anomaly-detection.md),
[app-collectors.md](app-collectors.md).

## How it fits together

```text
 node                                      paqtra-api
 ┌──────────────────────────────┐          ┌──────────────────────────────────┐
 │ paqtra agent (DaemonSet)     │  gzip    │ ingest hub: one store per node   │
 │  collectors ──► local store  │  JSON    │  tier 0  1 s    (1 h)            │
 │  anomaly bits    (replay     │ ───────► │  tier 1  1 min  (14 d)           │
 │                   buffer)    │  POST    │  tier 2  1 h    (365 d)          │
 └──────────────────────────────┘          │ Hubble flows ──► node "hubble"   │
                                           │ anomaly summary · metric alerts  │
                                           │ exporters · WebSocket live feed  │
                                           └──────────────────────────────────┘
```

- The agent posts batches to `POST /api/v1/agents/metrics` with the shared
  `X-Paqtra-Agent-Key` header. When the API is unreachable it keeps up to
  `PAQTRA_METRICS_BUFFER_SECONDS` of history and replays it when the API comes
  back. If the API reports a gap, the agent resends from the API's last
  sample. A batch carries up to 500,000 points; each batch repeats the
  metadata of every series in it, so large batches are what let a node with
  ~20k series clear a backlog faster than it collects (about 15 minutes of
  backlog in a minute on the lab node).
- The API keeps one store per node under `PAQTRA_METRICS_DIR` (default
  `${PAQTRA_DATA_DIR}/metrics`). Tier 0 chunks use Gorilla compression; tiers 1
  and 2 keep min, max, sum, count and the anomalous count per bucket, so
  averages and anomaly rates hold over long windows.
- Flows the API already receives from Hubble feed a separate store reported as
  node `hubble`, so cluster-wide flow, verdict, drop, HTTP and DNS series sit
  next to the node metrics.

## What is collected

| Section | Contexts (examples) | Source |
|---|---|---|
| System | `system.cpu`, `system.load`, `system.ram`, `system.processes`, `cpu.cpu`, `mem.*`, `system.softnet_stat` | `/proc/stat`, `/proc/loadavg`, `/proc/meminfo`, `/proc/vmstat`, PSI |
| Disks and filesystems | `disk.io`, `disk.util`, `disk.await`, `disk.space`, `disk.inodes` | `/proc/diskstats`, `statvfs` under the host root |
| Network | `net.net`, `net.packets`, `net.drops`, `net.errors`, `net.operstate`, `ip.*`, `ipv4.*`, `ipv6.*`, `netfilter.conntrack*` | `/proc/net/*`, `/sys/class/net` |
| Workloads | `cgroup.cpu`, `cgroup.mem`, `cgroup.throttled`, `cgroup.io` per pod | cgroup v2 / v1 files, labelled with namespace, pod, workload and workload kind from the Kubernetes API |
| Process groups | `app.cpu_utilization`, `app.mem_usage`, `app.processes` | `/proc/<pid>/stat` and `status`, grouped by the kernel `comm` only |
| Applications | `apps.up`, `redis.*`, `nginx.*`, `<kind>.*` | status endpoints, see [app-collectors.md](app-collectors.md) |
| Hubble | `hubble.flows`, `hubble.policy_verdicts`, `hubble.drop_reasons`, `hubble.http`, `hubble.dns` | flows already ingested by the API |

Privacy rules:

- The process collector reads `comm` (the 15-byte kernel name). It never
  reads `cmdline` or `environ`.
- Pod metadata comes from a pod list for the agent's own node
  (`spec.nodeName=<node>`), refreshed every 30 s. Finished pods
  (`Failed`/`Succeeded`, including evicted ones) are excluded server-side and
  the list is paged 250 pods at a time, so thousands of leftover pods on a
  node do not inflate the agent's memory. The agent never reads Secrets.
- No payloads are collected.

Hubble series are per node (`hubble.flows`, `hubble.policy_verdicts`,
`hubble.drop_reasons`) or per namespace (`hubble.http`, `hubble.dns`), with
the namespace as a label.

## Querying

| Method and path | Purpose |
|---|---|
| `GET /api/v1/metrics/nodes` | Nodes with a store, their last ingest and storage stats. |
| `GET /api/v1/metrics/contexts?filter=&nodes=` | Contexts with their charts, labels and dimensions. |
| `GET /api/v1/metrics/data` | One context as a time series (below). |
| `GET /api/v1/metrics/anomalies` / `correlations` | See [anomaly-detection.md](anomaly-detection.md). |
| `GET /api/v1/metrics/alerts` | See [metric-alerts.md](metric-alerts.md). |
| `GET /api/v1/metrics/status` | Ingest auth mode, Hubble feed, detector, alerts and exporters. |
| `GET /api/v1/metrics/exporters` | Exporter status. |
| `GET /api/v1/ws/metrics/live?token=` | WebSocket: send `{"subscribe":[query,...]}` (up to 50), receive `{"results":{id:result}}` every second. |

`/metrics/data` parameters:

| Parameter | Meaning |
|---|---|
| `context` | Required, for example `system.cpu`. |
| `nodes`, `charts`, `dimensions` | Comma-separated globs. |
| `labels` | `k=v,k2=v2`; values are globs. |
| `after`, `before` | Unix seconds or negative seconds relative to now (default `-600`, `0`). |
| `points` | Output points (default 300, max 5000). |
| `group_by` | `dimension` (default), `chart`, `node`, `instance`, `all` or `label:<key>`. |
| `aggregate` | How grouped series combine: `sum`, `avg`, `min`, `max`. |
| `group` | Bucket function: `avg` (default), `min`, `max`, `sum`, `last`. |
| `tier` | Force a tier; by default the finest tier that covers the window is used. |

Each output dimension carries `values` and an `anomalyRate` per point.

### CLI

```bash
paqtra metrics status
paqtra metrics nodes
paqtra metrics contexts --filter net
paqtra metrics query system.cpu --after -900 --group-by node
paqtra metrics top
paqtra metrics anomalies --after -3600 --top 20
paqtra metrics alerts [--all]
```

`--api` / `PAQTRA_API_URL` (default `http://127.0.0.1:9191`) and
`--api-token` / `PAQTRA_API_TOKEN` select the API; `-o json` prints raw JSON.

## Console

- **Node Metrics** (`/node-metrics`): metric families grouped by section, one
  canvas chart per context or instance, live once a second for windows up to
  an hour (polling beyond that). Each chart has an anomaly ribbon; drag across
  a chart to highlight a range.
- **Metric Anomalies** (`/metric-anomalies`): anomaly rate per node over time
  and the most anomalous dimensions. Drag across the timeline to rank the
  metrics that changed.
- **Metric Alerts** (`/metric-alerts`): raised alerts, history, silences and
  the loaded rules, with ack and silence for editors.

The older **Scorecard** page (`/metrics`) is unchanged.

## Exporting

The API can push every series to one or more destinations at a chosen
resolution. Exporters track progress per series, so a restart or outage
resends the missed range (up to the tier's retention) instead of dropping it.

| Variable | Meaning |
|---|---|
| `PAQTRA_METRICS_REMOTE_WRITE_URL` | Prometheus remote write endpoint (snappy-compressed protobuf). |
| `PAQTRA_METRICS_REMOTE_WRITE_HEADERS` | `Name: value, Name2: value2`, for example an `Authorization` header. |
| `PAQTRA_METRICS_OTLP_ENDPOINT` | OTLP/HTTP JSON endpoint; `/v1/metrics` is appended when missing. |
| `PAQTRA_METRICS_OTLP_HEADERS` | Same format as above. |
| `PAQTRA_METRICS_OTLP_RESOURCE` | Extra resource attributes, `k=v,k2=v2`. |
| `PAQTRA_METRICS_GRAPHITE_ADDR` | `host:port` for the Graphite plaintext protocol. |
| `PAQTRA_METRICS_EXPORT_RESOLUTION` | Seconds per exported point (default 10). |
| `PAQTRA_METRICS_EXPORT_CONTEXTS` / `_EXCLUDE` | Context globs to include or exclude. |
| `PAQTRA_METRICS_EXPORT_PREFIX` | Metric name prefix (default `paqtra`). |
| `PAQTRA_CLUSTER_NAME` | Added as the `cluster` label / resource attribute. |

Metric names are `<prefix>_<context>` with dots replaced by underscores
(for example `paqtra_system_cpu`). Each series carries `instance` (the node),
`chart`, `dimension` and `family` labels plus its chart labels.

## Configuration

### Agent

| Variable | Default | Meaning |
|---|---|---|
| `PAQTRA_METRICS` | `true` | Turn collection off with `false`. |
| `PAQTRA_METRICS_API` | empty | API base URL. Empty keeps metrics local (served at `/metrics/status` on the agent port). |
| `PAQTRA_AGENT_KEY` | empty | Shared ingest key; must match the API. |
| `PAQTRA_METRICS_INSECURE_TLS` | `false` | Skip TLS verification towards the API. |
| `NODE_NAME` | hostname | Node name for the stream. |
| `PAQTRA_METRICS_PROC`, `_SYS`, `_FS_ROOT`, `_CGROUP_ROOT` | `/proc`, `/sys`, none, `/sys/fs/cgroup` | Host paths; the chart sets `/host/...`. |
| `PAQTRA_METRICS_PROCESS_GROUPS` | `64` | Process groups kept (by CPU); `0` disables the collector. |
| `PAQTRA_METRICS_CONTAINERS` | `false` (chart: `true`) | Chart each container cgroup as well as each pod cgroup. Roughly triples cgroup series. |
| `PAQTRA_METRICS_APPS_FILE` | empty | Static app collectors, see [app-collectors.md](app-collectors.md). |
| `PAQTRA_METRICS_APP_DISCOVERY` | `true` | Scrape annotated pods on this node. |
| `PAQTRA_METRICS_ANOMALY` | `true` | Agent-side anomaly models. |
| `PAQTRA_METRICS_BUFFER_SECONDS` | `3600` | Local history (minimum 300) used for replay. |
| `MALLOC_ARENA_MAX` | chart: `2` | glibc allocator arenas. Unbounded, per-thread arenas roughly doubled the agent's RSS on a 12-core node. |

### API

| Variable | Default | Meaning |
|---|---|---|
| `PAQTRA_METRICS_DIR` | `${PAQTRA_DATA_DIR}/metrics` | Store directory. Without a directory, tiers live in memory only. |
| `PAQTRA_METRICS_TIER0_SECONDS` | `3600` | Per-second retention. |
| `PAQTRA_METRICS_TIER1_SECONDS` | `1209600` | Per-minute retention (14 days). |
| `PAQTRA_METRICS_TIER2_SECONDS` | `31536000` | Per-hour retention (365 days). |
| `PAQTRA_METRICS_DISK_QUOTA_MB` | `1024` | Disk cap per store; the oldest rollup files go first. |
| `PAQTRA_AGENT_KEY` | empty | Required for ingest unless auth is disabled. |

### Helm

```yaml
auth:
  agentKey: ""          # empty: generated once and kept across upgrades
agent:
  metrics:
    enabled: true
    hostAccess: true    # hostNetwork + hostPID + / mounted read-only at /host
    processGroups: 64
    containers: true    # per-container cgroup charts as well as per-pod
    appDiscovery: true
    bufferSeconds: 3600
    apps: []
  resources:
    requests: {memory: 128Mi, cpu: 100m}
    limits: {memory: 512Mi, cpu: "1"}
api:
  metrics:
    diskQuotaMB: 128    # per store; keep the total under api.persistence.size
    alerts: {enabled: true, defaults: true, rulesDir: ""}
    export:
      remoteWriteURL: ""
      otlpEndpoint: ""
      graphiteAddr: ""
      headersSecret: ""  # keys remote-write-headers / otlp-headers
global:
  clusterName: ""
```

With `hostAccess`, the agent shares the host network and PID namespaces and
mounts `/` read-only at `/host` (propagation `HostToContainer`). The mount is
needed to read host interfaces, disks and pod cgroups. Without it, the agent
reports only what its own container can see.

## Sizing

Agent cost grows with the number of series, which is dominated by cgroups
(about 40 series per pod, ~3x that with `containers: true`), interfaces and
scraped applications. Measured on a 12-core lab node with ~130 live pods,
16 scraped apps and ~21k series:

| | Agent | API (ingest for that node) |
|---|---|---|
| CPU | 300–450m | ~230m |
| Memory | ~200 MiB with the 1-hour buffer full | grows with tier-0 retention |

- The chart's agent limits are 512Mi and 1 CPU. The previous 200m CPU limit
  throttled 80% of CPU periods and the sender fell behind; 256Mi was
  OOM-killed. Lower them only on small nodes, and check
  `GET /metrics/status` on the agent: `sender.cursor` should stay within a
  few seconds of now and collectors should report `skipped: 0`.
- To shrink a large node: set `containers: false`, lower `processGroups`, or
  lower `bufferSeconds` (memory scales with it).

## Limits

- Metrics live in the API process. Run one API replica (the default when
  `api.persistence.enabled`); with several replicas behind the Service, each
  replica holds only the batches it received.
- The Hubble-derived series start when the API starts receiving flows and
  reflect Hubble's sampling, not every packet.
- Process groups are keyed by `comm`, so two programs with the same 15-byte
  name share a group.
