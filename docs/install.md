# Install, build, deploy and test

Moved from the README.

## Quick start

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
paqtra connectivity test
paqtra sysdump            # redacted support bundle
```

Full reference: [docs/cli.md](cli.md).

### Build from source

Needs Rust and Node.js (see [Requirements](#requirements)).

```bash
git clone https://github.com/zyvorai/paqtra.git
cd paqtra

# Build everything
cargo build --release                    # TUI binary
cd web-api && cargo build --release      # API server
cd ../web-ui && npm ci && npm run build  # Web dashboard

# Run the TUI
./target/release/paqtra tui              # or: paqtra tui --skip-bootstrap

# Or deploy to a remote K3s cluster
./scripts/deploy-k3s-test.sh <host> <user> <password> --test
```

See [QUICKSTART.md](../QUICKSTART.md) for the Docker Compose and manual web-stack paths.

## Deployment options

| Method | Command |
|--------|---------|
| **Remote K3s** | `./scripts/deploy-k3s-test.sh <host> <user> <pass> --test` |
| **Docker** | `docker compose -f deployments/docker-compose.yaml up` |
| **CLI (recommended)** | `paqtra install` (chart built in; see [docs/cli.md](cli.md)) |
| **Helm** | `helm install paqtra oci://ghcr.io/zyvorai/charts/paqtra --version <X.Y.Z>` or `./chart` from a checkout |
| **CLI binary** | `curl -fsSL https://raw.githubusercontent.com/zyvorai/paqtra/main/install.sh \| sh`, or `./install.sh --from-source` |

### Environment variables

```bash
PAQTRA_HOST=0.0.0.0        # API listen address
PAQTRA_PORT=9191           # API port
JWT_SECRET=<min-32-chars>  # Required for auth
ADMIN_PASSWORD=<min-12>    # Required when auth is enabled
ADMIN_USERNAME=admin       # Optional (default: admin)
HUBBLE_ADDRESS=localhost:4245
UI_DIST_DIR=/var/lib/paqtra/ui  # Path to built web UI
AUTH_DISABLED=true         # Dev mode only (requires ENVIRONMENT=development|test)
```

## Testing

Run the same gates contributors use before a PR:

```bash
make check-all
make test-all
make build-all
```

CI also exercises the chart, CLI and remote smoke scripts under `scripts/ci-*.sh` when those jobs are enabled. Individual suites: `cargo test`, `cd web-api && cargo test`, `cd web-ui && npm test`.

## Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| Rust | 1.75 | latest stable |
| Node.js | 18 | 22+ |
| Kubernetes | 1.25 | 1.28+ |
| Cilium | 1.14 | 1.19+ |
| Redis | 6.0 | 7.0+ |
