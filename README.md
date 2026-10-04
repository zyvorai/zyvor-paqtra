# Paqtra

[![CI](https://github.com/zyvorai/zyvor-paqtra/actions/workflows/ci.yml/badge.svg)](https://github.com/zyvorai/zyvor-paqtra/actions/workflows/ci.yml)
[![License: Apache 2.0](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Cilium](https://img.shields.io/badge/cilium-1.14%2B-purple.svg)](https://cilium.io/)
[![Changelog](https://img.shields.io/badge/changelog-Keep%20a%20Changelog-informational.svg)](CHANGELOG.md)

[![Book a demo](https://img.shields.io/badge/Book_a_demo-0071e3?style=for-the-badge)](https://zyvor.dev/schedule?utm_source=github&utm_medium=paqtra&utm_campaign=readme_hero)
[![30-day PoC](https://img.shields.io/badge/30--day_PoC-1d1d1f?style=for-the-badge)](https://zyvor.dev/poc?utm_source=github&utm_medium=paqtra&utm_campaign=readme_hero)

![Paqtra — Cilium-native network observability and operations for Kubernetes](docs/social/paqtra-hero-dark.jpg)

### Why can't A reach B? Paqtra traces every flow and shows Cilium's verdict.

**Cilium-native network observability and operations for Kubernetes — trace every flow.**

**Web dashboard** · **REST API** · **Terminal TUI** · **Free, Apache 2.0** · **Read-only toward the datapath**

📖 **[Read the full docs](https://zyvorai.github.io/zyvor-paqtra/)** — quickstart, architecture, the Cilium boundary, and a product tour.

![Paqtra dashboard — Overview](docs/ux/00-overview.png)

## Why Paqtra

| When this happens… | Paqtra gives you… |
|---|---|
| A request fails and nobody knows why | Path explain and **Why denied?** on any flow, with confidence-tagged evidence |
| Cilium drops packets and the reason is buried | Drop analytics by Cilium reason, plus a per-packet explanation |
| You want to change policy without breaking things | Simulate a `CiliumNetworkPolicy` against indexed flows before you apply it |

Paqtra is observe-first. Flows come from Hubble; node-local enrichment may read the BPF map inventory. **Enforcement stays with Cilium**: policy changes go through `CiliumNetworkPolicy` (CNP), never through Paqtra's own datapath.

![How Paqtra works — Hubble flows in, answers out, Cilium enforces](docs/ux/readme-how-it-works.jpg)

## See it live

![Paqtra live demo](docs/ux/paqtra-live-demo.gif)

![Capabilities at a glance — Observe, Investigate, Secure, Operate](docs/ux/readme-capabilities.jpg)

Captured against a live lab cluster with Cilium and Hubble. Full tour on the [docs site gallery](https://zyvorai.github.io/zyvor-paqtra/gallery/).

## Observe

Live Hubble flows with verdict coloring and WebSocket streaming, a service map and an observed-traffic topology built from real flows. Real L7 DNS query/rcode/latency when Cilium DNS visibility is on; L4-only never invents SERVFAIL. [Capabilities →](docs/capabilities.md)

![Hubble flows with verdict coloring](docs/ux/01-flows.png)

![Service map from observed flows](docs/ux/04-service-map.png)

![Observed-traffic topology](docs/ux/05-topology.png)

## Investigate

Ask "why can't A reach B?" and get an answer with **observed**, **inferred** or **unavailable** evidence labels. Click **Why denied?** on any dropped flow. [Investigate →](docs/investigate.md) · [Root cause →](docs/rootcause.md)

![Path investigation — why can't A reach B?](docs/ux/02-investigate.png)

![Drop analytics by Cilium reason](docs/ux/03-drops.png)

![Diagnostics](docs/ux/08-diagnostics.png)

The dashboard also includes real kernel eBPF data views powered by `bpftool`: conntrack tables, policy maps, IP cache, LB maps and drop analytics. All read-only. [eBPF integration →](docs/ebpf-integration.md)

![eBPF map and program inventory (read-only)](docs/ux/07-ebpf.png)

## Secure

A visual rule builder, a YAML editor, ML-powered **AutoPolicy** and an evidence-backed simulator, all applied as `CiliumNetworkPolicy` through the Kubernetes API. Cilium enforces. [AutoPolicy →](docs/autopolicy.md) · [Simulator →](docs/simulator.md)

![Policies](docs/ux/06-policies.png)

Also included: compliance audits (CIS, NIST, SOC 2), anomaly detection, chaos engineering, canary deployments and multi-cluster views. [Everything Paqtra does →](docs/capabilities.md)

## What Paqtra never does

![Paqtra observes, Cilium decides — what it reads, what it never does](docs/ux/readme-boundary.jpg)

Hard boundaries, enforced by review and repeated in [AGENTS.md](AGENTS.md):

- Never writes Cilium BPF maps or pins over `cil_*` programs.
- Never attaches, detaches or replaces Cilium (or Netra) programs. Attachment inventory is **read-only** classification (`cil_*` / `netra_*` / other).
- The agent observes, reports health and inventories. Policy apply goes through Cilium CRDs.
- Does not collect application payloads, argv/cmdline, or Secret contents.
- Does not introduce a second CNI or compete with Cilium's datapath.
- Prefers Hubble for flows; uses maps for node-local enrichment only.

Details: [docs/cilium-brotherhood.md](docs/cilium-brotherhood.md) and [docs/ebpf-integration.md](docs/ebpf-integration.md).

## Paqtra vs PacketWolf

![Paqtra vs PacketWolf — what the commercial platform adds](docs/social/paqtra-vs-packetwolf-card.jpg)

Paqtra is the free, Apache-2.0 community edition. **PacketWolf** is Zyvor's commercial platform on the same Cilium-native foundation, for teams that need to go from *seeing* the network to *securing and operating* it.

| | **Paqtra** (Apache 2.0) | **PacketWolf** (commercial) |
|---|---|---|
| Flows, verdicts, service map, drops, root cause | ✅ | ✅ |
| AutoPolicy, simulator, healer, chaos, canary, replay, multi-cluster | ✅ | ✅ |
| Which **process** opened each socket (kernel attribution, syscall tracing, `netpred explain`) | — | ✅ |
| Custom eBPF probes with honest attach status | — (read-only by design) | ✅ opt-in |
| **Threat detection**: learned baselines, DGA and beaconing, attack graph, GeoThreat, Tetragon correlation | — | ✅ |
| **Containment**: six profiles, gated auto-response with rollback, forensic pack | — | ✅ |
| Zero-Trust Pilot, policy insight scorecard, blast radius, drift scan, KubePosture | — | ✅ |
| **Ask Zyra** network copilot (58 read-only tools, gated write actions) | — | ✅ |
| Compliance Lens (SOC 2, PCI-DSS, HIPAA), CVE posture, cost-aware egress, risk forecast | Basic audits (CIS, NIST, SOC 2) | ✅ |
| Kubernetes operator and CRDs, `netpred` CLI | — | ✅ |
| OIDC, SAML, LDAP sign-in, tenant views, SIEM export | JWT | ✅ |
| KubeVirt VM console and VNC, in-browser shell, podman/docker visibility | — | ✅ |
| Support | Community | ZyvorAI Labs |

Full breakdown: [docs/paqtra-vs-packetwolf.md](docs/paqtra-vs-packetwolf.md). For a demo, a trial or pricing, book a [demo](https://zyvor.dev/schedule?utm_source=github&utm_medium=paqtra&utm_campaign=readme_footer), start a [30-day PoC](https://zyvor.dev/poc?utm_source=github&utm_medium=paqtra&utm_campaign=readme_footer) or contact [sales@zyvor.dev](mailto:sales@zyvor.dev) or visit [zyvor.dev](https://zyvor.dev/?utm_source=github&utm_medium=paqtra&utm_campaign=readme_footer).

## Paqtra or Netra?

**Netra** is the independent eBPF sibling: its own programs and maps under `/sys/fs/bpf/netra`, with leased emergency control. The two must not fight.

| Choose **Paqtra** when… | Choose **Netra** when… |
| --- | --- |
| Cilium is already the CNI of record | You need CNI-independent observe on cgroup v2 alone |
| You want Hubble flows, path investigation and policy preview in one place | You want a leased emergency deny with automatic return to observe |
| Policy changes should stay in Cilium CNPs | You need kernel drop attribution and packet capture without a CNI |

Who owns what: [Suite placement](docs/suite-placement.md) · [Cilium boundaries](docs/cilium-brotherhood.md).

## Get started in a minute

Prerequisites: a Kubernetes cluster and a kube-context. Cilium with Hubble Relay is required too; `paqtra install --with-cilium` sets it up if it is missing.

```bash
# Install the CLI (downloads, verifies and installs the latest release)
curl -fsSL https://raw.githubusercontent.com/zyvorai/paqtra/main/install.sh | sh   # or: brew install zyvorai/tap/paqtra

# Install Paqtra into the current kube-context (the Helm chart is built in)
paqtra install            # add --with-cilium if the cluster has no Cilium yet
paqtra status --wait
paqtra ui                 # open the console

# Something wrong?
paqtra doctor
```

Build from source, Docker, Helm, environment variables and testing: **[Install guide](docs/install.md)** · CLI reference: [docs/cli.md](docs/cli.md).

## Security

JWT authentication (HS256, 32+ char secret) with no default credentials, RBAC-ready middleware, input validation on all kubectl-bound fields, an explicit CORS origin allowlist, rate limiting and confirmation dialogs on destructive operations.

Report vulnerabilities through [SECURITY.md](SECURITY.md). Deeper reading: [docs/client/security-whitepaper.html](docs/client/security-whitepaper.html).

## Documentation map

| I want to… | Read |
|---|---|
| See everything Paqtra does | [Capabilities and stack](docs/capabilities.md) · [Features](docs/features.md) · [Overview](docs/overview.md) |
| Understand the architecture | [Architecture](docs/architecture.md) · [Web architecture](docs/web-architecture.md) |
| Install, build and deploy | [Install guide](docs/install.md) · [QUICKSTART.md](QUICKSTART.md) · [Deploy options](docs/web-deployment.md) |
| Call the API | [REST API](docs/rest-api.md) · [OpenAPI](docs/openapi.yaml) · [API reference](docs/client/api-reference.html) |
| Use the terminal UI | [TUI](docs/tui.md) |
| Browse the repository | [Repository layout](docs/repository-layout.md) |
| Compare with PacketWolf | [Paqtra vs PacketWolf](docs/paqtra-vs-packetwolf.md) |
| Contribute | [CONTRIBUTING.md](CONTRIBUTING.md) · [AGENTS.md](AGENTS.md) · [CHANGELOG.md](CHANGELOG.md) |

## License

Commercial subscriptions and support: see [docs/SUBSCRIPTION-MODEL.md](docs/SUBSCRIPTION-MODEL.md).

Licensed under the **[Apache License 2.0](LICENSE)**. Contributions are accepted under the same license. See [NOTICE](NOTICE).

Built on [Cilium](https://cilium.io/), [Hubble](https://docs.cilium.io/en/stable/observability/hubble/), [Ratatui](https://ratatui.rs/), [Axum](https://github.com/tokio-rs/axum), [React](https://react.dev/) and [Tailwind CSS](https://tailwindcss.com/).

**Want the commercial platform?** [Book a demo](https://zyvor.dev/schedule?utm_source=github&utm_medium=paqtra&utm_campaign=readme_footer) · [30-day PoC](https://zyvor.dev/poc?utm_source=github&utm_medium=paqtra&utm_campaign=readme_footer) · [sales@zyvor.dev](mailto:sales@zyvor.dev)
