# Flow insights

Four read-only views derived from the flows Paqtra already stores. They answer
questions the flow list cannot: is an SLO being burned right now, which edges
are failing and why, what a Hubble `labelsContext` would cost in Prometheus,
and how to hand flows to an OpenTelemetry collector.

All of them read the flow store, honour namespace scoping and rate limits like
the other `/api/v1/flows` reads, and change nothing in the cluster. They do not
read Hubble, touch Cilium, or feed policy apply. Research and rationale:
[RESEARCH.md](RESEARCH.md).

A shared caveat: flows reach Paqtra by polling Hubble, so every ratio below is
over the sampled flows, not every request. Each response says so.

## Parameters shared by the flow-history views

| Parameter | Meaning |
|---|---|
| `from`, `to` | RFC 3339 range, `to` exclusive. Default: the hour before now. At most 30 days. |
| `namespace` | Either side is in this namespace. |
| `pod`, `port`, `verdict` | As for `/api/v1/flows/history`. |

Every response carries `range`, `coverage` (how far back the store goes) and
`capture` (how flows were collected), the same as `/api/v1/flows/history`.

## SLO burn rate

`GET /api/v1/slo/targets/{id}/burn-rate`, CLI `paqtra flows burn-rate <id>`,
MCP `paqtra_slo_burn_rate`.

The SLO list shows a status projected from the latest flow sample. Burn rate
measures the error budget being spent over real windows of stored history.
Burn rate is the error ratio (dropped / (forwarded + dropped)) divided by the
error budget (1 - target). The tiers are the Google SRE Workbook's
recommendation (chapter 5, table 5-8):

| Severity | Long window | Short window | Burn rate | Budget used at threshold (30 d SLO) |
|---|---|---|---|---|
| page | 1 h | 5 min | 14.4 | 2% |
| page | 6 h | 30 min | 6 | 5% |
| ticket | 3 d | 6 h | 1 | 10% |

A tier fires only when both its windows exceed the threshold, so a recovered
incident stops alerting as soon as the short window is clean. `status` is
`page`, `ticket`, `ok` or `insufficient_data`.

Honesty rules:

- A window the stored history does not fully reach back over is not judged:
  its tier is `insufficient_history`. A fresh install cannot page on 3 days.
- A window with fewer than 20 judged flows is `insufficient_data`, not `ok`.
- Window edges snap to 5-minute buckets. Buckets dated in the future (clock
  skew) are ignored.
- Other verdicts (audit, traced, redirected) are not judged.
- It reports; it never applies policy or silences anything.

## Service edges with golden signals

`GET /api/v1/flows/history/edges?granularity=workload&max_edges=200`, CLI
`paqtra flows edges`, MCP `paqtra_flow_edges`.

Who talks to whom, per edge (source, destination, protocol): flows, flows per
second, forwarded / dropped counts, drop ratio, the top three Cilium drop
reasons and the busiest ports; and per service the inbound and outbound drop
ratio. `granularity` is `namespace`, `workload` (default; the owner name
guessed from the pod name, so replicas of a Deployment, DaemonSet or
StatefulSet are one node) or `pod`.

Latency and saturation are not in L3/L4 flow records and are not guessed:
`not_measured` says so. A drop ratio is Cilium dropping flows, not an
application error over a forwarded connection.

Bounded by design, so a scan cannot blow up the answer:

- The store groups in SQL and returns at most 50,000 groups, busiest first;
  `truncated: true` means quieter ones were left out and counts are lower
  bounds.
- At most `max_edges` (1 to 1000) edges and services are listed; the rest are
  added up in `folded` so totals still reconcile.
- Endpoints with no namespace or pod (world, host, kube-apiserver) become one
  node, `external`; their IPs are never node names.
- Per edge only the top ports are listed, with `port_count` showing how many
  distinct ports were seen (a port scan is visible, not enumerated).
- Labels are cut to 253 characters, control characters removed, protocol names
  reduced to a short alphanumeric label.

## Hubble metric cardinality advisor

`GET /api/v1/flows/history/cardinality?budget=20000`, CLI `paqtra flows
cardinality`, MCP `paqtra_metric_cardinality`.

Hubble's `flow` handler exposes `hubble_flows_processed_total{protocol, type,
subtype, verdict}` and the `drop` handler `hubble_drop_total{reason,
protocol}`; `labelsContext` adds source and destination labels
([Cilium metrics docs](https://docs.cilium.io/en/stable/observability/metrics/)).
Each distinct combination is a Prometheus series. The advisor counts, from the
flows in the range, how many series each choice would have produced:

| Context | `labelsContext` |
|---|---|
| none | (empty) |
| namespace | `source_namespace,destination_namespace` |
| workload | `source_namespace,source_workload,destination_namespace,destination_workload` |
| pod | `source_namespace,source_pod,destination_namespace,destination_pod` |

Each context gets a risk (`low` under a quarter of the budget, `ok`, `high`
over budget, `extreme` over ten times), and `recommended` names the most
detailed context that fits. `budget` is the series you can afford for the two
handlers together (100 to 10,000,000, default 20,000).

Read the figures as follows:

- The `flow` handler also splits by `type` and `subtype`, which flow records
  do not carry: its count is a lower bound.
- IP contexts are not offered; they have at least as many series as pod
  contexts.
- Hubble removes series tied to a deleted pod a minute later, so live pod
  series follow the pods running now; the advisor's count covers every pod seen
  in the range and is an upper bound for that context.
- The advisor suggests; it does not change Cilium or Hubble configuration.

## OpenTelemetry log export

`GET /api/v1/flows/history/otlp?limit=1000`, MCP `paqtra_flows_otlp`.

Returns `{ range, records, skipped_no_timestamp, limit, otlp }` where `otlp` is
an OTLP/JSON logs document (`resourceLogs` with one resource per cluster:
`service.name=paqtra`, `k8s.cluster.name`). Each flow is one log record,
newest first, at most 1000:

| Attribute | From |
|---|---|
| `source.address`, `destination.address` | flow IPs, only when they parse as IP addresses |
| `network.type` | `ipv4` or `ipv6` from the source address |
| `destination.port` | flow port, when non-zero |
| `network.transport` | `tcp` or `udp`; other protocols are left to `paqtra.protocol` |
| `k8s.src.namespace`, `k8s.src.name`, `k8s.src.owner.name` (and `dst`) | pod and namespace; owner name is the workload guess |
| `cilium.verdict`, `cilium.drop_reason` | Cilium verdict and, for drops, its reason |
| `paqtra.protocol`, `paqtra.flow.id` | raw protocol and flow id |

Dropped flows have severity WARN (13), the rest INFO (9). Flows without a
usable timestamp are skipped and counted. Paqtra sends nothing: feed the
document to a collector (for example with the `otlpjson` receiver) or POST it
to an OTLP/HTTP logs endpoint yourself. `network.*` and `source.*` /
`destination.*` follow OpenTelemetry semantic conventions; the `k8s.src.*` names
follow Grafana Beyla's network flow metric; `cilium.*` and `paqtra.*` are
Paqtra's own.

## CLI

```bash
paqtra flows edges --namespace prod --granularity workload --max-edges 30
paqtra flows cardinality --budget 50000 --from 2026-11-20T00:00:00Z
paqtra flows burn-rate slo-1a2b3c4d
paqtra flows edges -o json        # the API reply
```

`--api` / `PAQTRA_API_URL` and `--api-token` / `PAQTRA_API_TOKEN` select the
API, as for `paqtra metrics`.
