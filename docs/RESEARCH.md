# Research: Cilium and Kubernetes network observability

Date: 2026-11-23. Purpose: find what comparable tools do, what Paqtra lacks,
and which gaps fit Paqtra's boundaries (`AGENTS.md`: Hubble-first, read-only
toward the datapath, enforcement stays with Cilium, no payloads).

Evidence marking. A claim is plain only when it was read from a fetched page
in this research pass. **[U]** means it comes from a web-search result snippet
or from memory and was not read on the source page: treat it as a lead, not a
fact. Pages actually fetched:

- Cilium, "Monitoring & Metrics" (Hubble metrics by handler, `labelsContext`,
  cardinality): <https://docs.cilium.io/en/stable/observability/metrics/>
- Google SRE Workbook, "Alerting on SLOs", table 5-8:
  <https://sre.google/workbook/alerting-on-slos/>

## What the tools do

| Tool | What it does | Source |
|---|---|---|
| Hubble metrics (Cilium) | Handlers `flow`, `drop`, `tcp`, `dns`, `http`/`httpV2`. `hubble_flows_processed_total{protocol,type,subtype,verdict}`, `hubble_drop_total{reason,protocol}`. `labelsContext` adds source/destination `ip`, `namespace`, `pod`, `workload`, `workload_kind`, `app`. Series tied to a deleted pod are removed one minute after it goes. L7 metrics only for pods with L7 visibility. | Cilium metrics docs (fetched) |
| Hubble UI / Relay / CLI | Service map built from live flows, denied edges drawn red, per-namespace view. The enterprise edition adds a policy editor that builds a policy from live flows. | <https://isovalent.com/blog/post/cilium-hubble-enterprise-part-2> [U], <https://cilium.readthedocs.io/en/stable/gettingstarted/hubble/> [U] |
| Microsoft Retina | CNI-agnostic eBPF metrics at node and pod level (drops, TCP, UDP, DNS), flow logs, distributed packet captures bundled with iptables and socket stats, started from CLI or a CRD. | <https://azure.microsoft.com/en-us/blog/microsoft-open-sources-retina-a-cloud-native-container-networking-observability-platform/> [U] |
| Inspektor Gadget | Per-node privileged DaemonSet of eBPF "gadgets"; `top tcp` ranks connections by traffic, `trace tcp` shows connect/accept/close. Also runs without Kubernetes. | <https://github.com/inspektor-gadget/inspektor-gadget> [U] |
| Grafana Beyla | Network flow metric `beyla.network.flow.bytes` with source/destination address, port and Kubernetes attributes (`k8s.src.namespace`, `k8s.src.owner.name`, ...); default `socket_filter` attach so it does not conflict with Cilium's tc programs; `cidrs` filter. | <https://grafana.com/docs/beyla/latest/network> [U] |
| Coroot | Service map and per-service request rate, errors and latency from eBPF; availability SLO default 99%, latency SLO as the share of requests under a threshold. | <https://docs.coroot.com/inspections/slo> [U], <https://coroot.com/ebpf> [U] |
| Pixie | In-cluster eBPF protocol tracing with full request bodies, data kept in the cluster, short retention. | <https://github.com/pixie-io/pixie> [U] |
| Kentik Kube | eBPF agent feeding a SaaS; flow sampling guidance for routers (NetFlow/IPFIX/sFlow) is in rate terms, not documented for the Kube agent. | <https://www.kentik.com/blog/kentik-kube-extends-network-observability-to-kubernetes-deployments> [U], <https://kb.kentik.com/docs/flow-overview.md> [U] |
| OpenShift Network Observability | eBPF flows, default sampling 1 in 50, export to IPFIX, Kafka or OpenTelemetry. | <https://docs.redhat.com/en/documentation/openshift_container_platform/4.20/html/network_observability/configuring-network-observability-operators> [U] |
| OpenTelemetry semantic conventions | `network.transport`, `network.peer.address`, `network.peer.port` are stable; the "choosing network attributes" guidance is still in development. | <https://opentelemetry.io/docs/specs/semconv/registry/attributes/network/> [U] |
| SRE Workbook burn-rate alerts | Burn rate is how fast the service consumes the error budget relative to the SLO. For a 99.9% SLO: page at 14.4x over 1h with a 5m short window (2% of budget), page at 6x over 6h / 30m (5%), ticket at 1x over 3d / 6h (10%). | SRE Workbook (fetched) |

## What Paqtra already has

Flow store with time index and read deadlines, service map and dependency
views, drop analytics, evidence-backed path investigation, policy simulation,
AutoPolicy, a per-second metrics platform with OTLP / remote-write export, SLOs
(availability only, **projected from the latest flow sample, not from history**),
MCP with 39 read-only tools. Paths in `docs/capabilities.md`, `docs/metrics.md`.

## What Paqtra lacks (gaps found)

1. SLOs have no measured history. The existing evaluation says so itself
   ("projected from the latest N flows"). The industry pattern for alerting on
   an SLO is multiwindow, multi-burn-rate; Paqtra cannot say "paging-worthy
   burn" versus "slow leak".
2. The service map shows edges but not golden signals per edge: no traffic
   rate, no drop ratio with Cilium's drop reasons, no bounded behaviour under
   scans (one node per source IP, one edge per port).
3. Nothing helps choose Hubble's `labelsContext`. The Cilium docs name the
   knobs and that `source_pod` style contexts multiply series, but a cluster
   owner has to guess; the usual failure is Prometheus memory.
4. No standards-shaped export of flows. The metrics platform exports metrics;
   flows themselves cannot be handed to an OpenTelemetry collector with
   network attributes.
5. The MCP `paqtra_flow_history` and `paqtra_flow_timeline` tools had no
   `from`/`to`/`verdict` parameters although the API takes them, so an agent
   could only ask about the last hour.
6. Not pursued (see Rejected): packet capture orchestration, protocol tracing,
   flow-level sampling controls.

## Ranked ideas

Ranking is value over risk against the boundaries, with implementation cost
noted. Built = shipped in this change.

| # | Idea | Why | Fit and risk | Status |
|---|---|---|---|---|
| 1 | SLO burn rate over stored flow history, SRE Workbook tiers | Closes the largest honesty gap in SLOs; turns "projected" into "measured"; the alert shape is a documented standard | Pure read of the flow store. Notify-only, never wired to policy (consistent with the metrics platform boundary). Needs history coverage checks | Built |
| 2 | Service edges with golden signals, bounded cardinality | Hubble UI shows the map, Coroot shows RED; flows alone can give rate and drop ratio honestly. Bounded output is what makes it safe on a scanned cluster | Pure read. Latency and saturation are not in L3/L4 flows, so they are reported as not measured | Built |
| 3 | Hubble metric cardinality advisor | Prometheus cost is the #1 operational complaint about per-pod Hubble metrics; the cluster's own flows can answer "what would this cost" | Pure read, advisory only; does not change Cilium or Hubble config | Built |
| 4 | OTLP/JSON flow export with OpenTelemetry network attributes | Interop with any collector; follows stable `network.*` conventions and Beyla's `k8s.src.*` names | Pure read; Paqtra sends nothing. Cilium verdict has no convention so it uses a `cilium.*` namespace | Built |
| 5 | Flow-derived SLO for latency / HTTP status | Coroot-style latency SLO | Only possible where L7 visibility is on; flow store does not keep HTTP code or latency today. Needs schema work first | Deferred |
| 6 | Sampling and retention policy per namespace (keep drops longer than forwards) | Pixie/Retina-style cost control | Changes what the store keeps: a data-loss decision that needs an owner and migration | Deferred |
| 7 | Cilium Network Policy editor from live flows | Isovalent enterprise feature | Paqtra already has AutoPolicy, the visual builder and the simulator | Already covered |
| 8 | NetFlow/IPFIX export | Router-world interop | Flows from Hubble are sampled polls, not exporter-grade counters; OTLP covers the modern path | Rejected for now |
| 9 | Distributed packet capture (Retina) | Strong debugging aid | Needs privileged capture; `AGENTS.md` forbids payload collection | Rejected |
| 10 | eBPF protocol tracing with bodies (Pixie) | Deep L7 visibility | Collects application payloads: forbidden by `AGENTS.md` | Rejected |
| 11 | Attach own eBPF network flow programs (Beyla/Retina style) | Independent of Hubble | Second observer on the datapath; `AGENTS.md`: Hubble first, never attach over Cilium | Rejected (Netra's territory) |

## Design notes from the sources

- Cardinality: Cilium documents that `labelsContext` entries each become a
  label even when empty, and that pod-tied series expire a minute after pod
  deletion. The advisor therefore reports observed distinct series for the
  window as an upper bound for pod contexts, and says so.
- The advisor offers `namespace`, `workload` and `pod` contexts and not IP
  contexts: an IP context has at least as many series as a pod context.
- Burn-rate windows use the workbook table for a 99.9% SLO as fixed tiers;
  the budget share per tier is computed against the SLO's own window, so
  14.4x for 1h of a 30d window prints 2%, matching the table.
- Because Hubble flows reach Paqtra by polling, ratios are over sampled flows.
  Every new view carries that statement.
