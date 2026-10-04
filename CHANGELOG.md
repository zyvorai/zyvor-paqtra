# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Per-second metrics platform.** The agent collects host, CPU, memory, PSI,
  disk, filesystem, interface, IP/TCP/UDP, conntrack, pod cgroup and
  process-group metrics every second, keeps a local replay buffer and streams
  gzip batches to `POST /api/v1/agents/metrics`, authenticated by a shared
  `PAQTRA_AGENT_KEY` (generated and kept by the Helm chart). The API stores
  them per node in Gorilla-compressed 1 s / 1 min / 1 h tiers with a disk
  quota and serves `/api/v1/metrics/{nodes,contexts,data,status}` plus a
  one-second WebSocket feed. Collectors are read-only; the process collector
  reads `comm` only, never `cmdline` or `environ`. See `docs/metrics.md`.
- **Hubble-derived series.** Flows the API already ingests become
  `hubble.flows`, `hubble.policy_verdicts`, `hubble.drop_reasons`,
  `hubble.http` and `hubble.dns` under node `hubble`.
- **Anomaly detection.** Per-dimension k-means models flag samples on the
  agent; `/api/v1/metrics/anomalies` ranks nodes and dimensions, and
  `/api/v1/metrics/correlations` ranks what changed in a window
  (Kolmogorov–Smirnov). See `docs/anomaly-detection.md`.
- **Metric alerts.** A Netdata-style rule engine with 45 built-in rules (host,
  disk, network, TCP, conntrack, pods, apps, Hubble drops, denials, HTTP 5xx,
  DNS errors, anomaly rate), hysteresis, delays, repeat, ack and silences,
  delivered through the existing notifier as `metric:<rule>`. Alerts never
  apply policy. See `docs/metric-alerts.md`.
- **Application collectors.** nginx, Apache, HAProxy, Redis, Memcached, Envoy,
  CoreDNS, etcd and any Prometheus endpoint, from a static file or pod
  annotations (`paqtra.io/app-kind`, `prometheus.io/scrape`) on the agent's
  node. Credentials come only from env vars named in config. See
  `docs/app-collectors.md`.
- **Exporters.** Prometheus remote write, OTLP/HTTP and Graphite, with
  per-series progress so outages are backfilled.
- **Console and CLI.** Node Metrics, Metric Anomalies and Metric Alerts pages
  with live canvas charts, and `paqtra metrics
  {status,nodes,contexts,query,top,anomalies,alerts}`.

### Changed

- **Helm agent host access.** With `agent.metrics.hostAccess` (default on) the
  agent DaemonSet uses the host network and PID namespaces and mounts `/`
  read-only at `/host` so node metrics describe the host.
- **web-ui TypeScript pinned back to 5.9.** The Dependabot bump to TypeScript
  7 broke `npm ci` (typescript-eslint peer range), ESLint and `tsconfig`
  `baseUrl`.

### Fixed

- **API no longer hangs at startup on a large flow backlog.** Flow-history
  retention deleted every expired row in one statement before the server
  bound its port, and again after every ingest batch; on a spinning disk a
  multi-GB `flows.db` kept the pod unready until the liveness probe killed it.
  Retention now runs on a background thread, 500 rows per chunk through the
  time index with a pause between chunks, never on the startup or ingest path.
- **Flow ingest no longer freezes the API on a slow disk.** SQLite inserts
  ran on the async workers; under a 1-CPU limit tokio has one, so a stalled
  insert stopped every request and the liveness probe restarted the pod.
  Inserts now run on the blocking pool.
- **Metrics catch-up outpaces collection.** The API stores each series'
  points under one lookup instead of cloning metadata per point, and agent
  batches carry up to 500k points (metadata is repeated per batch); at ~20k
  series the old path only kept pace and a backlog never cleared.
- **Agent memory.** The agent sets `MALLOC_ARENA_MAX=2` (glibc per-thread
  arenas doubled its RSS on a 12-core node) and its default limits are 512Mi
  and 1 CPU: ~20k series with the hour-long buffer settle near 250Mi and
  ~300m, and the old 200m limit throttled the sender until it fell behind.
- **Agent pod list is bounded.** The metrics pod index listed every pod ever
  scheduled on the node, including evicted ones (12k on one lab node, 150 MB
  of JSON), and was OOM-killed. It now skips `Failed`/`Succeeded` pods
  server-side and pages the list 250 pods at a time.

## [2.2.2] - 2026-09-27

### Fixed

- **A recent-window flow query no longer scans the namespace index.** With no
  statistics, SQLite read `idx_flows_path` and sorted the result for any query naming
  a source and destination namespace, ignoring the time bound: on a 12M-flow store the
  connectivity monitor's two-minute lookups each hit the 5s read deadline every cycle,
  holding the read connection and leaving every store-backed endpoint 10-17s slow, and
  `paqtra connectivity test` failing with a 500. Queries (and their counts and
  timelines) whose window is at most a day now read through the time index, so cost
  follows the window, not the filters.
- **The Overview dashboard's large flow counts no longer wrap mid-number.** A local
  `Metric` component formatted big values with `toLocaleString()` instead of the
  already-imported `compact()` helper, so an 8-digit flow count (e.g. `20,635,325`)
  wrapped across two lines inside its fixed-width tile. It now renders compacted
  (`20.6M`), matching every other large figure on the page.

## [2.2.1] - 2026-09-26

### Fixed

- **A large flow store no longer stalls the API.** On a host with 10.6M stored flows
  (4.5 GB), a flows query filtered by namespace made the API stop answering until the
  scan finished, minutes later. The flow store's `stats()` ran two full-table scans
  under its single lock on every call, reads shared the ingest connection, and a
  namespace filter has no usable index. Now: `stats()`/`lag_secs()` never wait (last
  known value when busy; totals by rowid span above 100k rows), reads use a separate
  `query_only` connection so they cannot block ingest, and every read has a deadline
  (5s, `PAQTRA_FLOW_QUERY_TIMEOUT_SECS`) after which the API falls back to Hubble.
  Coverage of the whole store is an index lookup, not a scan.
- `paqtra connectivity test` checks flows through the time-bounded `/flows/history`
  endpoint, which stays cheap however large the store is.

## [2.2.0] - 2026-09-26

### Added

- **`paqtra` installs itself like `cilium-cli`.** The Helm chart is compiled into the
  binary (CLI vX installs chart vX and image tags vX); other versions are pulled from
  `oci://ghcr.io/zyvorai/charts`. `helm` is used from PATH or downloaded once
  (pinned version, sha256-verified) into `~/.paqtra/bin`.
  - `install`/`upgrade`: `--version`, `-f/--values`, `--set`, `--set-string`,
    `--set-file`, `--registry` (image mirror), `--dry-run` (server-side), `--atomic`,
    `--no-wait`, `--wait-duration`, `--list-versions`, `--with-cilium`
    (installs Cilium + Hubble Relay + metrics when none is running), and prerequisite
    checks (Kubernetes >= 1.25, Cilium >= 1.14, Hubble + Relay, RBAC, StorageClass).
  - Global `--context`, `--kubeconfig`, `-n/--namespace`, `--release`, `--helm-path`.
  - `status` reads the Kubernetes API (no `kubectl`), really waits with `--wait`,
    and exits non-zero when unhealthy. `version` compares client, release and server.
  - `hubble enable|disable|port-forward`, `ui`, `config view|get|set`,
    `completion bash|zsh|fish|powershell`, `uninstall --purge`.
- **Release pipeline**: static musl Linux and macOS binaries with `sha256sums.txt`, a
  cosign keyless signature, SBOM and the chart attached to the release; the chart, a
  multi-arch agent image (previously never published) and a `paqtra-cli` image are
  pushed; a Homebrew formula is rendered. `install.sh` is now a verifying release
  installer (`curl | sh`); the build-from-source flow is `scripts/dev-install.sh`.
- **Rule-level policy editing**: `POST|PUT|DELETE /api/v1/policies/{id}/rules` add,
  replace or delete one rule of a CiliumNetworkPolicy through the CRD, with
  `resource_version` concurrency (409), `?dry_run=true`, audit entries
  (`policy.rule.*`), and the cluster's validation message as a 400. `GET
  /policies/{id}` now returns `spec`, `resource_version` and `rule_counts`. New
  **Policy Rules** page (`/policy-rules`) with templates for entities, CIDR sets,
  ICMP, FQDN, DNS and L7 HTTP rules. A policy's last rule cannot be deleted
  (Cilium rejects rule-less policies).
- **Alert rule CRUD**: `POST /alerts/rules`, `PUT /alerts/rules/{id}/definition`,
  `DELETE /alerts/rules/{id}`; conditions are validated against what the engine can
  evaluate; built-in rules need `?force=true`.
- **Cilium Insights** (`/cilium-insights`) and read-only endpoints:
  `/cilium/features` (from `cilium-config`), `/hubble/nodes` (`GetNodes`, buffer
  fill), `/hubble/metrics` and `/cilium/metrics` (Prometheus), `/cilium/resources/{kind}`
  (BGP v2, LB-IPAM, L2, pod IP pools, CIDR groups, Gateway API, ...),
  `/cilium/agent/{what}` (allow-listed `cilium-dbg` queries).
- Flows carry an optional `hubble` object (identities, labels, direction,
  `policy_match_type`, observation point, node); identities are persisted in the
  flow store.
- Chart value `api.env.prometheusUrl` (`PROMETHEUS_URL`); chart and manifest RBAC
  now grant read access to the Cilium CRDs the views use (nodes, egress gateway,
  BGP, LB-IPAM, L2, pod IP pools, CIDR groups, Gateway API).
- Quiet Hubble follow streams flush every **2s** (in addition to the 64-flow batch)
  so low-volume clusters persist evidence promptly.
- `GET /api/v1/changes/{id}/impact` — before/after flow correlation for Change Log
  (filters, chart, evidence-backed; gaps → `inconclusive`; not causation).
- Investigation bundle export + **time-limited share** with redacted incident card:
  `…/export`, `…/share`, `GET …/investigate/share/{token}`.
- Declared connectivity paths (observe-only): sustained multi-sample alerts,
  silence (`POST …/connectivity/alerts/{id}/silence`), evidence deep-links.
- Flow store ops: `GET /api/v1/flows/store`, admin `POST …/purge`; gap timeline on
  Cluster Health.
- Overview per-tile **8s timeouts** and **stale** badges.
- CI `RUN_LIVE` gates for connectivity, impact, investigate export/share, flows/store.

### Changed

- `GET /flows?verdict=` is filtered by Hubble itself, so `limit` returns the most
  recent flows with that verdict rather than the few matches among the last N.
- Renamed the invented `cilium.io/canary-*` and `cilium.io/mtu` annotations to
  `paqtra.io/*`; Cilium never read them. Docs no longer claim chaos loads eBPF programs.
- `/health` and `/ready` stay cheap via background sampler; cache SQLite I/O uses
  `spawn_blocking`; bpftool concurrency capped.
- Overview auto-refreshes every 15s; `/ebpf/summary` defaults to counts-only
  (`?detail=full` for CT dumps); `/cluster/health` runs kubectl probes in parallel
  with a short cache.
- Chart API memory defaults: request **512Mi**, limit **2Gi** (durable flow index
  + bpftool inventory need headroom; 512Mi limits were OOM-killing under load).
- Connectivity list stays cheap; per-path status is
  `GET /api/v1/connectivity/paths/{id}/status`. Change-impact / status queries use
  `spawn_blocking` so SQLite does not stall the async runtime.

### Fixed

- `/ebpf/drops` and `/ebpf/summary` counted forwarded traffic as drops (only
  `cilium_metrics` reasons >= 130 are drops), read four key bytes instead of the
  reason byte, missed per-CPU values, and used invented reason names; now decoded
  correctly and named with Hubble's `DropReason`.
- Five `kubectl exec -l k8s-app=cilium` calls (invalid) now resolve an agent pod
  first: ClusterMesh, WireGuard/encryption and NAT/CT counts returned nothing.
- Policy rule/apply failures no longer surface as "Internal server error".

## [2.1.0] - 2026-09-24

### Added

- Continuous Hubble **follow** ingest with disconnect/gap counters, events/sec, and
  lag on `/health` → `subsystems.flow_ingest`.
- Real DNS L7 fields on flows (query, rcode, answers, latency); DNS UI no longer
  invents SERVFAIL from policy drops.
- `POST /api/v1/investigate/flow` — why-denied workflow from a selected flow
  (identities, CNP/CCNP candidates, drop reason, draft allow). Flows UI button
  on DROPPED rows.

### Changed

- Chart default `HUBBLE_MODE=grpc`; ingest labels `hubble_grpc` when Observer gRPC
  produced the data.
- Combined image defaults Hubble Relay Service port **80**.
- README and product docs updated for gRPC follow ingest, DNS evidence, and
  deny explanation.

[2.2.1]: https://github.com/zyvorai/paqtra/releases/tag/v2.2.1
[2.2.0]: https://github.com/zyvorai/paqtra/releases/tag/v2.2.0
[2.1.0]: https://github.com/zyvorai/paqtra/releases/tag/v2.1.0

## [2.0.0] - 2026-09-24

### Added

- **Investigate Path** — `POST /investigate/path` returns a confidence-tagged
  path report (`observed` | `inferred` | `unavailable`) with evidence from
  Hubble flows, EndpointSlices, and Cilium policies. UI: Investigate Path view.
- SQLite flow store with Hubble ingest, query APIs, and offline golden CI
  (`ci-investigate-golden.sh`).
- Evidence-backed policy **simulate** (no speculative allow without flow or
  policy evidence).
- Hubble **Observer gRPC** client (`HUBBLE_MODE=grpc|cli|auto`); CLI remains a
  fallback. Chart defaults and lab deploy use the in-cluster Hubble Service.
- Helm chart persistence (`PAQTRA_DATA_DIR`), expanded ClusterRole for
  EndpointSlices / CCNP / events, and in-cluster kubectl via ServiceAccount.
- Docs: `docs/investigate.md`; AGENTS.md read-only boundary (no BPF attach).

### Changed

- CLI, API, UI, install script, and Helm chart aligned on **2.0.0**.
- Chart `auth.adminPassword` defaults empty (auto-generate). Lab demo pin
  `Admin@321` is applied only by `scripts/deploy-remote.sh`.
- Repository home [`zyvorai/paqtra`](https://github.com/zyvorai/paqtra);
  images under `ghcr.io/zyvorai/paqtra*`.
- Documentation restructured to flat kebab-case product docs.
- Full Apache License 2.0, `NOTICE`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, and
  GitHub community templates.

### Fixed

- In-cluster kubectl no longer targets `localhost:8080`; uses SA token/CA.
- Hubble health against Service port **80** (not container 4245).
- Flow JSON unwrap of Hubble `{"flow":...}` envelopes; IP source/destination
  parsing so Flows UI no longer shows UNKNOWN rows.
- Vite 8 Rolldown `manualChunks` and Tailwind 4 `@tailwindcss/postcss` for
  reliable UI builds.
- gRPC build via `tonic-prost-build`; TUI clippy clean-ups.

### Removed

- Gated `aya-ebpf` / map-writing / program-attach code that violated the
  read-only boundary (`AGENTS.md`). Map reads stay on `bpftool`.
- Session/status milestone docs under `docs/status/`.
- Obsolete Cilium-Vision branding and confidential client markings.

[2.0.0]: https://github.com/zyvorai/paqtra/releases/tag/v2.0.0
