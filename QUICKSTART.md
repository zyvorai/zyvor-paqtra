# Paqtra — Quick Start

**Paqtra — trace every flow.** Get up and running in minutes.

## Install into a cluster (recommended)

You need a Kubernetes cluster, a kube-context, and Cilium with Hubble Relay
(`paqtra install --with-cilium` sets Cilium up when it is missing).

```bash
# Install the CLI: downloads, verifies (sha256, and cosign if present) and installs
curl -fsSL https://raw.githubusercontent.com/zyvorai/paqtra/main/install.sh | sh
# or: brew install zyvorai/tap/paqtra

paqtra install            # the Helm chart is built in; add --with-cilium if needed
paqtra status --wait      # exits 0 only when everything is healthy
paqtra ui                 # open the console
```

Something wrong? `paqtra doctor` diagnoses it, `paqtra connectivity test` proves
policy enforcement end to end, and `paqtra sysdump` collects a redacted support
bundle. Full reference: [docs/cli.md](docs/cli.md).

## Build from source

| Component | Requirement |
|-----------|-------------|
| Rust | 1.75+ (TUI and web-api) |
| Node.js | 18+ with npm (20+ recommended for web-ui) |
| Docker | Optional, for containerised deployment |
| kubectl | Required for the TUI; cluster must have Cilium + Hubble enabled |

## Quick Start (TUI)

```bash
git clone https://github.com/zyvorai/paqtra.git
cd paqtra

cargo build --release
./target/release/paqtra tui
```

### Key Bindings

```
? = Help   q = Quit   Tab = Next tab
e = Explain packet          (Flows)
g = Generate  a = Apply    (AutoPolicy)
s = Simulate                (Simulator)
t = Time-travel             (Replay)
Enter = Run experiment      (Chaos)
p = Promote  r = Rollback  (Canary)
```

## Quick Start (Web Stack)

### Option A: Docker Compose (recommended)

```bash
# 1. Configure environment
cp deployments/.env.example deployments/.env
# Edit deployments/.env: set JWT_SECRET (≥32 chars) and ADMIN_PASSWORD (≥12 chars)

# 2. Start all services (API + UI + Redis)
make docker-up

# Combined stack (typical remote deploy): http://localhost:9191
# Compose UI-only mapping may also expose http://localhost:3001
# API: http://localhost:9191
```

To stop:

```bash
make docker-down
```

### Option B: Run manually

```bash
# Terminal 1 - build and run the API
cd web-api
cargo build --release
JWT_SECRET=change-me-to-a-real-secret-at-least-32-chars \
ADMIN_PASSWORD=change-me-to-a-strong-password \
  cargo run --release

# Terminal 2 - build and run the UI
cd web-ui
npm ci
npm run build     # production build in dist/
npm run dev       # or start the dev server on :3000 (proxies /api to :9191)
```

## Development

```bash
make dev          # TUI: format, check, build
make api-dev      # Web API dev server
make ui-dev       # Web UI Vite dev server (port 3000, proxies /api to :9191)
make test-all     # Run all tests (TUI + API + UI)
make check-all    # Lint/check all components
make build-all    # Production build of everything
```

## Kubernetes Deployment

Production-ready manifests live in `deployments/k8s/`. The directory contains Deployments, Services, Ingress, RBAC, ConfigMap, and Secrets resources for the full stack (API, UI, Redis).

```bash
kubectl apply -f deployments/k8s/
```

Review and update `deployments/k8s/secrets.yaml` with your own JWT secret before applying.

Or use the CLI, which has the chart built in: `paqtra install` (see [docs/cli.md](docs/cli.md)); from a checkout, `helm install paqtra ./chart`.

## Web UI Keyboard Shortcuts

```
/ = Search       ? = Shortcuts help
r = Refresh      Esc = Close modal
g h = Dashboard  g f = Flows  g t = Topology  g p = Policies
```

## Next Steps

- [docs/features.md](docs/features.md) — feature guide
- [docs/web-app.md](docs/web-app.md) — web UI
- [docs/web-architecture.md](docs/web-architecture.md) — system architecture
- [docs/web-deployment.md](docs/web-deployment.md) — deployment options
- [docs/cilium-brotherhood.md](docs/cilium-brotherhood.md) — Cilium / Netra boundaries
- [docs/overview.md](docs/overview.md) — product overview
