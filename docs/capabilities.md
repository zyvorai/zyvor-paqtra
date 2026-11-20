# Capabilities and stack

Full capability list, dashboard pages, policy management, TUI and the stack diagram. Moved from the README, which now carries the short pitch.

## What it does

| Area | Capabilities |
|------|-------------|
| **Flow monitoring** | Live Hubble Observer gRPC follow-stream into a local flow store, verdict coloring, WebSocket streaming, ingest gap visibility on `/health` |
| **Investigation** | Path explain (`POST /investigate/path`), **Why denied?** from a selected flow (`POST /investigate/flow`), confidence-tagged evidence |
| **DNS** | Real L7 DNS query/rcode/answers/latency when Cilium DNS visibility is on; L4-only never invents SERVFAIL |
| **Policy management** | Visual rule builder, YAML editor, ML-powered AutoPolicy, evidence-backed simulate before applying |
| **Security** | Zero-trust policy generation, compliance audits (CIS/NIST/SOC2), anomaly detection |
| **Root cause** | Packet drop analysis, policy correlation, one-click fixes, network healer |
| **Chaos engineering** | Fault injection (loss/latency/DNS), circuit breaker, preset experiments |
| **Canary deployments** | Progressive traffic shifting, health gates, auto-promote/rollback |
| **Multi-cluster** | Cluster registration, health checks, policy sync, topology view |
| **Networking** | Load balancer, ingress/egress, IPAM, encryption, BGP, ClusterMesh, WireGuard |
| **Observability** | Service map, heatmap, DNS monitor, latency analysis, bandwidth, cost analytics |
| **Operations** | Diagnostics, troubleshooter, packet capture sessions, SLOs, alerts, audit log, incidents |

The dashboard includes real kernel eBPF data views powered by `bpftool`: conntrack tables, policy maps, IP cache, LB maps and drop analytics. These are read-only; see [docs/ebpf-integration.md](ebpf-integration.md).

## Stack

```
+------------------------------------------------------------------+
|                    Web Dashboard (React 19 + TypeScript)           |
|  60+ pages | WebSocket | Dark/Light theme                          |
+------------------------------------------------------------------+
|                    REST API (Rust + Axum)                          |
|  OpenAPI-documented | JWT auth | Redis cache | Rate limiting       |
+------------------------------------------------------------------+
|                    Terminal TUI (Rust + Ratatui)                   |
|  13 tabs | Live monitoring | Packet explainer                      |
+------------------------------------------------------------------+
|  eBPF maps (read-only) | Hubble gRPC (Relay) | Kubernetes API | Redis |
+------------------------------------------------------------------+
|  bpftool (read-only kernel data: conntrack, policy, IP cache, LB)  |
+------------------------------------------------------------------+
|              Cilium agent  |  Linux kernel eBPF datapath           |
+------------------------------------------------------------------+
```

See [docs/architecture.md](architecture.md) and [docs/web-architecture.md](web-architecture.md).

## Web dashboard

60+ pages, grouped in the navigation as Overview · Investigate · Diagnostics · Security · Reports · Fleet. The routes and their descriptions live in [`web-ui/src/navConfig.ts`](../web-ui/src/navConfig.ts).

- **Investigate**: Path ("why can't A reach B?"), Flows (**Why denied?** on DROPPED rows), Endpoints, Identities, DNS (L7 when available), Capture, Topology, Service Map
- **Diagnostics**: Health (Hubble mode, ingest gaps/lag), latency, Drops, eBPF profiler and map explorer, Root Cause, Healer
- **Security**: Policies, Policy Rules (add/edit/delete single rules), Policy Editor, Visual Rule Builder, Templates, Anomalies, Compliance, RBAC
- **Intelligence**: AutoPolicy, Chaos Engineering, Canary Deployments
- **Operations**: Alerts, Audit Log, SLOs, Incident Timeline, Settings
- **Networking**: ClusterMesh, BGP, Load Balancer, Ingress/Egress Gateway, IPAM, Encryption, WireGuard

Every view has auto-refresh (30 s, with a toggle), a data-freshness indicator, CSV/JSON export, empty-state messaging and error handling with retry.

### Policy management

| Feature | How |
|---------|-----|
| **Create (YAML)** | Monaco editor with syntax highlighting and validation |
| **Create (Visual)** | Form-based rule builder with live YAML preview |
| **Create (Template)** | Pre-built policy templates library |
| **Create (ML)** | AutoPolicy engine learns traffic and generates policies |
| **Edit / update** | Click the pencil icon, modify, apply |
| **Delete** | Single or bulk (multi-select checkboxes) |
| **Simulate** | Dry-run impact analysis before applying |
| **Validate** | Schema validation with error details |

Policies are applied as `CiliumNetworkPolicy` objects through the Kubernetes API. Cilium enforces them.

## Terminal TUI

13 interactive tabs with vim-style navigation: Flows, Connections, Endpoints, Policies, Metrics, Healer, AutoPolicy, RootCause, Simulator, Replay, Chaos, Canary and MultiCluster.

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Cycle tabs |
| `?` | Help overlay |
| `q` | Quit |
| `e` | Explain selected packet |
| `d` | Detect problems (Healer) |
| `s` | Simulate / Stop |
| `t` | Enter time-travel mode |

## Security

- JWT authentication (HS256, 32+ char secret); no default credentials, `ADMIN_PASSWORD` is required when auth is enabled
- RBAC-ready middleware
- Input validation on all kubectl-bound fields
- CORS with an explicit origin allowlist
- Rate limiting
- Confirmation dialogs on destructive operations
- Secure temp files via `tempfile::NamedTempFile`

Report vulnerabilities through [SECURITY.md](../SECURITY.md). Deeper reading: [docs/client/security-whitepaper.html](client/security-whitepaper.html).
