# Metric alerts

`paqtra-api` evaluates threshold and anomaly-rate rules against the
per-second metrics store (see [metrics.md](metrics.md)) and sends every
transition through the existing notifier (Slack, webhook, PagerDuty and the
other channels configured for Paqtra alerts) as rule `metric:<rule>`.

A metric alert only notifies. It can raise, clear, repeat or be silenced. It
never applies, changes or suggests a Cilium policy. The only state the API can
change is notification state (ack and silence).

## Rule format

Rules are YAML documents with a top-level `alerts:` list. The syntax follows
Netdata's health configuration closely, so most Netdata alarms port with a
field rename.

```yaml
alerts:
  - alarm: cpu_usage_high          # unique name
    on: system.cpu                 # metric context
    dimensions: "*"                # globs, separated by space, comma or |
    lookup: average -10m           # <function> -<window>
    aggregate: sum                 # how matched dimensions combine
    units: "%"
    every: 1m
    warn: '$this > (($status >= $WARNING) ? 75 : 85)'
    crit: '$this > (($status == $CRITICAL) ? 85 : 95)'
    delay_down: 15m
    repeat: 6h
    class: Utilization
    info: average CPU utilization over the last 10 minutes
```

| Field | Required | Meaning |
|---|---|---|
| `alarm` | yes | Rule name. A later rule with the same name replaces an earlier one. |
| `on` | yes | Context, for example `net.net` or `hubble.http`. |
| `charts` | no | Chart ID globs. Empty or `*` matches all. |
| `dimensions` | no | Dimension globs. Empty or `*` matches all. |
| `labels` | no | Chart labels that must match (values are globs), for example `namespace: shop`. |
| `lookup` | yes | `<function> -<window>`. Functions: `average`/`avg`/`mean`, `min`, `max`, `sum`, `last`, `median`/`p50`, `p90`, `p95`, `p99`, `anomaly-rate`. |
| `aggregate` | no | `sum`, `avg`, `min` or `max` across matched dimensions. Default `avg` for percent units, `sum` otherwise. |
| `per` | no | Instance granularity: `chart` (default), `dimension` or `node`. |
| `vars` | no | Constants available as `$name`. |
| `calc` | no | Expression turning the lookup result into `$this`. |
| `warn`, `crit` | one of them | Expressions; true raises the status. `crit` wins. |
| `every` | no | Evaluation interval, default 10s. |
| `delay_up`, `delay_down` | no | How long a worse / better status must hold before it changes. |
| `repeat` | no | Re-notify while raised, unacknowledged and unsilenced. |
| `units`, `class`, `info` | no | Shown in the API, console and notifications. |
| `enabled` | no | `false` removes the rule, including a built-in one of the same name. |

Durations accept `90s`, `15m`, `6h`, `7d` or a bare number of seconds.

## Expression language

- numbers, `nan`, `inf`
- `+ - * / %` (division or modulo by zero gives NaN)
- `> >= < <= == !=`, `&& || !`, `?:`, parentheses
- `abs(x)`, `min(...)`, `max(...)`

Any comparison involving NaN is false, so a rule over missing data never
raises.

| Variable | Value |
|---|---|
| `$this` | The aggregated lookup result, or the `calc` result. |
| `$<dimension>` | That dimension's lookup result, for example `$responses` or `$errors_5xx`. |
| `$status` | `$UNDEFINED` (-1), `$CLEAR` (1), `$WARNING` (3), `$CRITICAL` (4). |
| `$anomaly_rate` | Percent of samples flagged anomalous in the window. |
| `$ncpu` | CPUs on the node. |
| `$now` | Unix seconds. |

Use `$status` for hysteresis: `warn: '$this > (($status >= $WARNING) ? 75 : 85)'`
raises above 85 and clears below 75.

## Built-in rules

The embedded pack (`web-api/paqtra-metrics/src/metricalert/defaults.yaml`,
45 rules):

| Area | Rules |
|---|---|
| CPU and load | `cpu_usage_high`, `cpu_iowait_high`, `cpu_steal_high`, `load_average_high`, `cpu_pressure_some` |
| Memory | `ram_in_use`, `oom_kill`, `memory_pressure_full`, `swap_used_high`, `swap_io_heavy` |
| Disk | `disk_space_usage`, `disk_inode_usage`, `disk_util_high`, `disk_await_high`, `io_pressure_full` |
| Interfaces | `interface_inbound_drops`, `interface_outbound_drops`, `interface_errors`, `interface_fifo_errors`, `interface_down`, `interface_carrier_changes`, `softnet_dropped`, `softnet_squeezed` |
| TCP and UDP | `tcp_listen_overflows`, `tcp_syn_queue_drops`, `tcp_syncookies_sent`, `tcp_backlog_drops`, `tcp_retransmit_timeouts`, `tcp_memory_pressure`, `udp_receive_buffer_errors`, `udp_send_buffer_errors` |
| Conntrack and system | `conntrack_table_full`, `conntrack_insert_failed`, `file_descriptors_high`, `processes_blocked` |
| Pods (cgroups) | `workload_cpu_throttling`, `workload_memory_near_limit`, `workload_oom_kill` |
| Applications | `app_down` |
| Hubble | `hubble_drop_ratio`, `hubble_policy_denied`, `hubble_http_5xx_ratio`, `hubble_dns_errors` |
| Anomaly | `node_anomaly_rate`, `network_anomaly_rate` |

Read the YAML for exact thresholds. To change one, add a rule with the same
`alarm` name in `PAQTRA_METRICALERT_DIR`; to drop one, set `enabled: false`.

## Silences and acknowledgement

- **Ack** stops repeat notifications for one raised instance until its next
  status change.
- **Silence** suppresses notifications for instances matching rule, node and
  chart globs until it expires. Status is still tracked and shown as
  `silenced`. Silences persist in `metricalert-silences.json` in the metrics
  directory. The notifier also honours existing silences for rule
  `metric:<rule>`.

Ack and silence need the editor role and are written to the audit log.

## API and CLI

| Method and path | Purpose |
|---|---|
| `GET /api/v1/metrics/alerts?all=&history=` | Raised instances (all with `all=true`), transitions, rules, silences, stats. |
| `POST /api/v1/metrics/alerts/{id}/ack` | Acknowledge a raised instance. |
| `POST /api/v1/metrics/silences` | Body: `rule`, `node`, `chart` globs, `duration` (seconds) or `until` (Unix seconds), `comment`. |
| `DELETE /api/v1/metrics/silences/{id}` | Remove a silence. |

```bash
paqtra metrics alerts [--all]
```

## Configuration

| Variable | Default | Meaning |
|---|---|---|
| `PAQTRA_METRICALERT` | `true` | `false` turns the engine off. |
| `PAQTRA_METRICALERT_DEFAULTS` | `true` | `false` skips the built-in pack. |
| `PAQTRA_METRICALERT_DIR` | empty | `*.yaml` / `*.yml` files loaded in name order on top of the defaults. |

An invalid rule disables the engine and reports the error in
`GET /api/v1/metrics/status` (`alertsError`) instead of running a partial
pack. Helm: `api.metrics.alerts`.
