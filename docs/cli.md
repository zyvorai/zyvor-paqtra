# The `paqtra` CLI

One binary installs, checks and operates Paqtra, the way `cilium` does for
Cilium. The Helm chart is built in, so nothing else needs to be downloaded by
hand.

## Install the CLI

```bash
# Linux and macOS (amd64 / arm64): downloads, verifies and installs the latest release
curl -fsSL https://raw.githubusercontent.com/zyvorai/paqtra/main/install.sh | sh

# a specific version, or somewhere other than /usr/local/bin
curl -fsSL https://raw.githubusercontent.com/zyvorai/paqtra/main/install.sh | sh -s -- --version 2.2.0 --install-dir ~/.local/bin

# Homebrew
brew install zyvorai/tap/paqtra
```

`install.sh` checks the download against the release's `sha256sums.txt` and
refuses to install on a mismatch or a missing checksum. If
[`cosign`](https://docs.sigstore.dev/cosign/) is installed it also verifies the
checksum file's signature. By hand:

```bash
V=2.2.0; OS=linux; ARCH=x86_64          # macos, aarch64 also exist
curl -fsSLO https://github.com/zyvorai/paqtra/releases/download/v$V/paqtra-$V-$OS-$ARCH.tar.gz
curl -fsSLO https://github.com/zyvorai/paqtra/releases/download/v$V/sha256sums.txt
curl -fsSLO https://github.com/zyvorai/paqtra/releases/download/v$V/sha256sums.txt.sigstore.json
sha256sum -c --ignore-missing sha256sums.txt
cosign verify-blob --bundle sha256sums.txt.sigstore.json \
  --certificate-identity-regexp '^https://github.com/zyvorai/paqtra/\.github/workflows/release\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com sha256sums.txt
```

Developers can build from a checkout with `./install.sh --from-source`.

## Install Paqtra into a cluster

```bash
paqtra install                 # uses the current kube-context
paqtra status --wait           # exits 0 only when Paqtra, Cilium and Hubble Relay are healthy
paqtra ui                      # port-forward the console and open it
```

`paqtra install` first checks the prerequisites and stops on a failure:

| Check | Fails when | Warns when |
|-------|-----------|-----------|
| Kubernetes | older than 1.25 | older than 1.28 |
| Cilium | not running, or older than 1.14 | older than 1.19, or agents not ready |
| Hubble | disabled in `cilium-config` | no `enable-hubble` key |
| Hubble Relay | missing or not ready | |
| RBAC | cannot create ClusterRoles / bindings / CiliumNetworkPolicies | |
| StorageClass | | none is default and persistence is on |

No Cilium yet? `paqtra install --with-cilium` installs it first, with Hubble,
Relay and metrics on (skipped if a Cilium is already running, whoever
installed it).

### Version model

CLI `vX` installs chart `vX`, whose default image tags are `vX`. Another version
comes from the OCI registry:

```bash
paqtra install --list-versions
paqtra install --version 2.1.0
```

### Common flags (`install` and `upgrade`)

| Flag | Meaning |
|------|---------|
| `--context`, `--kubeconfig`, `-n/--namespace`, `--release` | which cluster and release (global: work before or after the command) |
| `--version X.Y.Z` | chart/app version (default: this CLI's) |
| `-f/--values FILE`, `--set k=v`, `--set-string`, `--set-file` | Helm values; your `--set` beats `--registry` |
| `--registry PREFIX` | pull `paqtra-api`, `paqtra-ui`, `paqtra` from a mirror |
| `--dry-run` | render and validate against the cluster, change nothing |
| `--atomic` | roll back if it does not become ready |
| `--no-wait`, `--wait-duration 5m` | do not wait / how long to wait |
| `--skip-preflight` (install) | install even if a prerequisite check fails |
| `--reset-values` (upgrade) | drop the release's previous values |
| `--chart-directory DIR` | use a chart from disk instead of the built-in one |

`paqtra upgrade` keeps your values and takes new chart defaults
(`--reset-then-reuse-values` on Helm 3.14+).

### Helm

`helm` is used from `PATH` (3.8+), or from `--helm-path` / `$PAQTRA_HELM`. If
there is none, a pinned Helm is downloaded once into `~/.paqtra/bin`, verified
against a checksum built into the CLI. For air-gapped hosts pass
`--no-download-helm` (or `PAQTRA_NO_DOWNLOAD=1`) and provide `helm` yourself.

## Everyday commands

| Command | What it does |
|---------|--------------|
| `paqtra status [--wait] [-o json] [--local]` | component health; **exit 1 when unhealthy** |
| `paqtra doctor [-o json]` | prerequisites plus crash loops, OOM kills, missing agents, Hubble ingest lag, with a fix for each |
| `paqtra version [--client]` | client, installed release and running API versions, with a skew warning |
| `paqtra info` | context, namespace, Hubble address, node and agent counts |
| `paqtra config view [--all]` / `get KEY` / `set KEY=VALUE...` | the release's Helm values |
| `paqtra hubble enable [--no-relay] [--metrics LIST]` / `disable` | switch Hubble on the Cilium Helm release |
| `paqtra hubble port-forward [--port 4245]` | forward a local port to Hubble Relay |
| `paqtra ui [--port 8443] [--no-open]` | port-forward the Paqtra console |
| `paqtra uninstall [--purge] [--yes]` | remove the release; `--purge` also deletes its PVCs (and the namespace if empty) |
| `paqtra completion bash\|zsh\|fish\|powershell` | shell completion script |

## Diagnose and get support

```bash
paqtra doctor                  # what is wrong, and what to do about it
paqtra connectivity test       # does Cilium enforce policy, end to end?
paqtra sysdump                 # a redacted zip for a support ticket
```

### `paqtra connectivity test`

Creates namespace `paqtra-connectivity-test` with an echo server and short-lived
probe pods, then checks:

1. **baseline**: both clients connect with no policy.
2. **enforcement**: after a `CiliumNetworkPolicy` (applied through the CRD) only
   the allowed client connects and the other is blocked.
3. **restore**: deleting the policy restores connectivity.
4. **flows**: the API recorded a DROPPED flow for the blocked client and a
   FORWARDED flow for the allowed one. Needs a Paqtra API token
   (`--api-token` or `PAQTRA_API_TOKEN`); otherwise it is skipped, not failed.

Everything it creates is labelled `paqtra.io/connectivity-test` and removed
afterwards (also on Ctrl-C), unless `--no-cleanup`. `--test NAME` runs one
scenario, `--registry PREFIX` pulls the test image from a mirror, `-o json` is
for CI. Exit code 1 on any failure.

### `paqtra sysdump`

Writes `paqtra-sysdump-<timestamp>.zip` (mode 0600) with: the doctor report,
Paqtra and Cilium/Hubble Deployments, DaemonSets, Pods, Services, ConfigMaps,
PVCs and events, container logs (`--log-lines`, `--since`), `cilium-config`,
Cilium policies and nodes (`--no-policies` to skip), and the Helm release's
values and history.

- **Kubernetes Secrets are never read**, and `helm get manifest` (which would
  include rendered Secrets) is not collected.
- Values under keys such as `password`, `token`, `secret`, `apiKey`, `jwt` and
  `authorization`, `env` entries with such names, URLs with credentials, JWTs and
  `Bearer` tokens are replaced with `[REDACTED]`.
- The bundle still contains pod names, IPs and policy specs. **Review it
  before sharing.** A collector that fails is listed in
  `collection-errors.txt`; the rest of the bundle is still written.

## Exit codes and environment

| | |
|---|---|
| `0` | success / healthy |
| `1` | `status`, `doctor` or `connectivity test` found a problem; any command error |

| Variable | Same as |
|----------|---------|
| `PAQTRA_NAMESPACE`, `PAQTRA_RELEASE`, `PAQTRA_CONTEXT` | `-n`, `--release`, `--context` |
| `PAQTRA_HELM`, `PAQTRA_NO_DOWNLOAD` | `--helm-path`, `--no-download-helm` |
| `PAQTRA_CHART_DIR` | `--chart-directory` (an invalid path is an error, not ignored) |
| `PAQTRA_API_TOKEN` | `connectivity test --api-token` |

## Using it in CI

```bash
docker run --rm -v ~/.kube:/home/paqtra/.kube:ro ghcr.io/zyvorai/paqtra-cli:2.2.0 status --wait
```

The image carries `paqtra` and `helm`.
