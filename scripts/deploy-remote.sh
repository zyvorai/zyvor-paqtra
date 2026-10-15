#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Paqtra — Remote Kubernetes Deployment via SSH + rsync
# Builds container images on the host and installs via Helm into K3s/K8s.
# No host systemd units — Paqtra runs in-cluster only.
#
# Usage:
#   ./scripts/deploy-remote.sh <host> <user> [password] [options]
#   ./scripts/deploy-remote.sh 10.0.1.5 root --key
#   ./scripts/deploy-remote.sh 10.0.1.5 root mypass --uninstall
#   ./scripts/deploy-remote.sh --fleet hosts.txt
# ─────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
VERSION="2.2.3"
REMOTE_DIR=""  # Resolved after TARGET_USER is known

# Parse args
TARGET_HOST="${1:-}"
TARGET_USER="${2:-root}"
TARGET_PASS="${3:-}"
QUICK_MODE=false
K3S_MODE=false
UNINSTALL=false
FLEET_FILE=""
KEY_AUTH=false

for arg in "$@"; do
    case "$arg" in
        --quick|--k3s)  ;;  # legacy flags ignored — always Kubernetes
        --uninstall) UNINSTALL=true ;;
        --key)       KEY_AUTH=true; TARGET_PASS="" ;;
        --fleet)     FLEET_FILE="${4:-}" ;;
    esac
done

ok()   { echo "  ✅ $*"; }
fail() { echo "  ❌ $*"; }
info() { echo "  ℹ️  $*"; }
warn() { echo "  ⚠️  $*"; }

# ─── SSH / rsync wrappers with sshpass support ───────────────

SSH_OPTS="-o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10"

_ssh() {
    if [ -n "${TARGET_PASS}" ] && command -v sshpass &>/dev/null; then
        export SSHPASS="${TARGET_PASS}"
        sshpass -e ssh ${SSH_OPTS} "${TARGET_USER}@${TARGET_HOST}" "$@"
    else
        ssh ${SSH_OPTS} "${TARGET_USER}@${TARGET_HOST}" "$@"
    fi
}

_rsync() {
    local rsync_opts="-az --delete --progress"
    if [ -n "${TARGET_PASS}" ] && command -v sshpass &>/dev/null; then
        export SSHPASS="${TARGET_PASS}"
        rsync ${rsync_opts} -e "sshpass -e ssh ${SSH_OPTS}" "$@"
    else
        rsync ${rsync_opts} -e "ssh ${SSH_OPTS}" "$@"
    fi
}

# ─── Validate ────────────────────────────────────────────────

validate() {
    if [ -z "${TARGET_HOST}" ]; then
        echo "Usage: $0 <host> <user> [password] [--uninstall|--key]"
        echo ""
        echo "Deploys Paqtra into the remote Kubernetes cluster (Helm + container images)."
        echo ""
        echo "Examples:"
        echo "  $0 10.0.1.5 root mypassword              # Build images + Helm install"
        echo "  $0 10.0.1.5 root --key                    # SSH key auth"
        echo "  $0 10.0.1.5 root mypassword --uninstall   # Remove Helm release + images"
        echo "  $0 --fleet hosts.txt                      # Deploy to multiple hosts"
        exit 1
    fi

    if [ -n "${TARGET_PASS}" ] && ! command -v sshpass &>/dev/null; then
        warn "sshpass not found — install it for password auth, or use --key"
        warn "  Fedora/RHEL: sudo dnf install sshpass"
        warn "  Ubuntu:      sudo apt install sshpass"
        exit 1
    fi
}

# ─── Connectivity check ─────────────────────────────────────

check_connectivity() {
    info "Testing SSH connectivity to ${TARGET_USER}@${TARGET_HOST}..."
    if _ssh "echo ok" &>/dev/null; then
        ok "SSH connected to ${TARGET_HOST}"
    else
        fail "Cannot SSH to ${TARGET_HOST}"
        exit 1
    fi

    # Resolve remote home directory
    REMOTE_HOME=$(_ssh "echo \$HOME" 2>/dev/null | tr -d '\r')
    REMOTE_HOME="${REMOTE_HOME:-/home/${TARGET_USER}}"
    REMOTE_DIR="${REMOTE_HOME}/paqtra"
    info "Remote deploy directory: ${REMOTE_DIR}"
}

# ─── Sync project files ─────────────────────────────────────

sync_files() {
    info "Syncing project to ${TARGET_HOST}:${REMOTE_DIR}..."

    _ssh "mkdir -p '${REMOTE_DIR}'"
    _rsync \
        --exclude '.git' \
        --exclude 'node_modules' \
        --exclude 'target' \
        --exclude 'dist' \
        --exclude '*.vmdk' \
        --exclude '*.qcow2' \
        --exclude '*.iso' \
        "${PROJECT_DIR}/" \
        "${TARGET_USER}@${TARGET_HOST}:${REMOTE_DIR}/"

    ok "Synced to ${TARGET_HOST}:${REMOTE_DIR}"
}

# ─── Sync pre-built binaries (quick mode) ────────────────────

sync_binaries() {
    info "Syncing pre-built binaries..."

    # API binary
    if [ -f "${PROJECT_DIR}/web-api/target/release/paqtra-api" ]; then
        _rsync \
            "${PROJECT_DIR}/web-api/target/release/paqtra-api" \
            "${TARGET_USER}@${TARGET_HOST}:/tmp/paqtra-api"
        ok "API binary synced"
    else
        warn "API binary not found — run 'make api-build' first"
    fi

    # UI dist
    if [ -d "${PROJECT_DIR}/web-ui/dist" ]; then
        _rsync \
            "${PROJECT_DIR}/web-ui/dist/" \
            "${TARGET_USER}@${TARGET_HOST}:/tmp/paqtra-ui/"
        ok "UI files synced"
    else
        warn "UI not built — run 'make ui-build' first"
    fi

    # Install script
    _rsync "${PROJECT_DIR}/install.sh" "${TARGET_USER}@${TARGET_HOST}:/tmp/paqtra-install.sh"
}

# ─── Uninstall old version ───────────────────────────────────

uninstall_old() {
    info "Removing legacy host installs (systemd) if present..."
    _ssh bash <<'REMOTE'
systemctl stop paqtra-api paqtra-ui hubble-port-forward 2>/dev/null || true
systemctl disable paqtra-api paqtra-ui hubble-port-forward 2>/dev/null || true
sudo rm -f /usr/local/bin/paqtra-api
sudo rm -f /usr/lib/systemd/system/paqtra-api.service
sudo rm -f /usr/lib/systemd/system/paqtra-ui.service
sudo rm -f /usr/lib/systemd/system/hubble-port-forward.service
sudo systemctl daemon-reload 2>/dev/null || true
REMOTE
    ok "Legacy host units removed"
}

# ─── Setup K8s + Cilium + Hubble stack ─────────────────────

setup_k8s_stack() {
    info "Setting up K3s + Cilium + Hubble on ${TARGET_HOST}..."
    _ssh bash <<REMOTE
set -e

# ── K3s ──────────────────────────────────────────────────────
if ! command -v k3s &>/dev/null; then
    echo "  Installing K3s (no flannel, no kube-proxy, no traefik)..."
    curl -sfL https://get.k3s.io | INSTALL_K3S_EXEC="--disable=traefik --flannel-backend=none --disable-network-policy --disable-kube-proxy" sh -
    echo "  Waiting for K3s API..."
    for i in \$(seq 1 60); do
        sudo KUBECONFIG=/etc/rancher/k3s/k3s.yaml kubectl get nodes &>/dev/null && break
        sleep 2
    done
    echo "  ✅ K3s installed"
else
    echo "  ✅ K3s already installed"
fi

# User-readable kubeconfig (non-root cannot open /etc/rancher/k3s/k3s.yaml)
mkdir -p "\$HOME/.kube"
sudo cp /etc/rancher/k3s/k3s.yaml "\$HOME/.kube/config"
sudo chown "\$(id -u):\$(id -g)" "\$HOME/.kube/config"
chmod 600 "\$HOME/.kube/config"
export KUBECONFIG="\$HOME/.kube/config"

# ── Helm ─────────────────────────────────────────────────────
if ! command -v helm &>/dev/null; then
    echo "  Installing Helm..."
    curl -fsSL https://raw.githubusercontent.com/helm/helm/main/scripts/get-helm-3 | bash
    echo "  ✅ Helm installed"
fi

# ── Cilium CLI ───────────────────────────────────────────────
if ! command -v cilium &>/dev/null; then
    echo "  Installing Cilium CLI..."
    CILIUM_CLI_VERSION=\$(curl -s https://raw.githubusercontent.com/cilium/cilium-cli/main/stable.txt)
    CLI_ARCH=amd64
    if [ "\$(uname -m)" = "aarch64" ]; then CLI_ARCH=arm64; fi
    curl -L --fail --remote-name-all "https://github.com/cilium/cilium-cli/releases/download/\${CILIUM_CLI_VERSION}/cilium-linux-\${CLI_ARCH}.tar.gz{,.sha256sum}"
    sha256sum --check cilium-linux-\${CLI_ARCH}.tar.gz.sha256sum 2>/dev/null || true
    sudo tar xzvf cilium-linux-\${CLI_ARCH}.tar.gz -C /usr/local/bin
    rm -f cilium-linux-\${CLI_ARCH}.tar.gz{,.sha256sum}
    echo "  ✅ Cilium CLI installed"
fi

# ── Cilium CNI ───────────────────────────────────────────────
if ! kubectl get daemonset -n kube-system cilium &>/dev/null; then
    echo "  Installing Cilium CNI with Hubble..."
    helm repo add cilium https://helm.cilium.io/ 2>/dev/null || true
    helm repo update cilium 2>/dev/null || true
    CILIUM_CHART_VERSION=1.17.3
    helm install cilium cilium/cilium --version \${CILIUM_CHART_VERSION} --namespace kube-system \
        --set operator.replicas=1 \
        --set kubeProxyReplacement=true \
        --set hubble.enabled=true \
        --set hubble.relay.enabled=true \
        --set hubble.ui.enabled=false \
        --set hubble.metrics.enabled="{dns,drop,tcp,flow,icmp,http}" \
        --wait --timeout 300s
    echo "  ✅ Cilium + Hubble installed"
else
    echo "  ✅ Cilium already installed"

    # Ensure Hubble relay is enabled (might be missing from older installs)
    HUBBLE_RELAY=\$(kubectl get deploy -n kube-system hubble-relay 2>/dev/null | grep -c hubble-relay || true)
    if [ "\$HUBBLE_RELAY" = "0" ]; then
        echo "  Enabling Hubble relay..."
        CILIUM_CHART_VERSION=\$(helm list -n kube-system -o json 2>/dev/null | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d[0]["chart"].split("-")[-1])' 2>/dev/null || echo "1.17.3")
        helm upgrade cilium cilium/cilium --version \${CILIUM_CHART_VERSION} --namespace kube-system --reuse-values \
            --set hubble.enabled=true \
            --set hubble.relay.enabled=true \
            --set hubble.metrics.enabled="{dns,drop,tcp,flow,icmp,http}" \
            --wait --timeout 120s 2>/dev/null || true
        echo "  ✅ Hubble relay enabled"
    else
        echo "  ✅ Hubble relay already running"
    fi
fi

# ── Wait for Cilium + Hubble to be ready ─────────────────────
echo "  Waiting for Cilium pods to be ready..."
kubectl -n kube-system wait --for=condition=ready pod -l k8s-app=cilium --timeout=120s 2>/dev/null || true
kubectl -n kube-system wait --for=condition=ready pod -l k8s-app=hubble-relay --timeout=120s 2>/dev/null || true

# ── Verify ───────────────────────────────────────────────────
echo ""
echo "  === Cluster Status ==="
kubectl get nodes
echo ""
echo "  === Cilium Status ==="
cilium status --brief 2>/dev/null || kubectl -n kube-system get pods -l k8s-app=cilium
echo ""
echo "  === Hubble Relay ==="
kubectl -n kube-system get pods -l k8s-app=hubble-relay
echo ""
REMOTE
    ok "K8s + Cilium + Hubble stack ready"
}

# ─── Kubernetes deployment (images + Helm) ───────────────────

deploy_kubernetes() {
    info "Deploying Paqtra into Kubernetes on ${TARGET_HOST}..."

    setup_k8s_stack
    uninstall_old

    _ssh bash <<REMOTE
set -e
cd ${REMOTE_DIR}
mkdir -p "\$HOME/.kube"
sudo cp /etc/rancher/k3s/k3s.yaml "\$HOME/.kube/config"
sudo chown "\$(id -u):\$(id -g)" "\$HOME/.kube/config"
chmod 600 "\$HOME/.kube/config"
export KUBECONFIG="\$HOME/.kube/config"
export PATH="\$HOME/.local/bin:/usr/local/bin:\$PATH"

RUNTIME=podman
command -v podman >/dev/null || RUNTIME=docker
if ! command -v "\$RUNTIME" >/dev/null; then
    echo "Need podman or docker to build images"
    exit 1
fi

ARCH=\$(uname -m)
case "\$ARCH" in
    aarch64|arm64) TARGETARCH=arm64 ;;
    *) TARGETARCH=amd64 ;;
esac

API_IMAGE=localhost/paqtra-api:latest
UI_IMAGE=localhost/paqtra-ui:latest
AGENT_IMAGE=localhost/paqtra:latest

echo "Building API image (\$RUNTIME, arch=\$TARGETARCH)..."
\$RUNTIME build --build-arg TARGETARCH=\$TARGETARCH -t "\$API_IMAGE" -f web-api/Dockerfile web-api/

echo "Building UI image..."
\$RUNTIME build -t "\$UI_IMAGE" -f web-ui/Dockerfile web-ui/

echo "Building agent/CLI image..."
\$RUNTIME build -t "\$AGENT_IMAGE" -f Dockerfile .

# Install host management CLI (like cilium-cli) from the image we just built.
echo "Installing paqtra CLI to /usr/local/bin..."
EXTRACT_CTR="paqtra-cli-extract-\$\$"
\$RUNTIME rm -f "\$EXTRACT_CTR" >/dev/null 2>&1 || true
\$RUNTIME create --name "\$EXTRACT_CTR" "\$AGENT_IMAGE" >/dev/null
\$RUNTIME cp "\$EXTRACT_CTR:/usr/local/bin/paqtra" /tmp/paqtra-cli
\$RUNTIME rm -f "\$EXTRACT_CTR" >/dev/null 2>&1 || true
sudo install -m 755 /tmp/paqtra-cli /usr/local/bin/paqtra
rm -f /tmp/paqtra-cli
# Chart for \`paqtra install\` / upgrade (PAQTRA_CHART_DIR or default share path)
sudo mkdir -p /usr/share/paqtra
sudo rm -rf /usr/share/paqtra/chart
sudo cp -a "${REMOTE_DIR}/chart" /usr/share/paqtra/chart
echo "  ✅ paqtra CLI \$(/usr/local/bin/paqtra version 2>/dev/null || echo installed)"

echo "Importing images into k3s..."
\$RUNTIME save "\$API_IMAGE" | sudo k3s ctr images import -
\$RUNTIME save "\$UI_IMAGE" | sudo k3s ctr images import -
\$RUNTIME save "\$AGENT_IMAGE" | sudo k3s ctr images import -

sudo k3s ctr images tag "\$API_IMAGE" docker.io/library/paqtra-api:latest 2>/dev/null || true
sudo k3s ctr images tag "\$UI_IMAGE" docker.io/library/paqtra-ui:latest 2>/dev/null || true
sudo k3s ctr images tag "\$AGENT_IMAGE" docker.io/library/paqtra:latest 2>/dev/null || true

NS=paqtra
kubectl get ns "\$NS" >/dev/null 2>&1 || kubectl create namespace "\$NS"

JWT=\$(openssl rand -hex 32)

echo "Installing Helm release..."
helm upgrade --install paqtra ${REMOTE_DIR}/chart \
    --namespace "\$NS" \
    --create-namespace \
    --set global.namespace="\$NS" \
    --set api.image.repository=localhost/paqtra-api \
    --set api.image.tag=latest \
    --set api.image.pullPolicy=Never \
    --set ui.image.repository=localhost/paqtra-ui \
    --set ui.image.tag=latest \
    --set ui.image.pullPolicy=Never \
    --set agent.enabled=true \
    --set agent.image.repository=localhost/paqtra \
    --set agent.image.tag=latest \
    --set agent.image.pullPolicy=Never \
    --set api.replicas=1 \
    --set ui.replicas=1 \
    --set api.hpa.enabled=false \
    --set monitoring.enabled=false \
    --set api.env.hubbleAddress=hubble-relay.kube-system.svc.cluster.local:80 \
    --set api.env.jwtSecret="\$JWT" \
    --set auth.adminUsername=admin \
    --set auth.adminPassword=Admin@321 \
    --set api.persistence.enabled=true \
    --set api.service.type=NodePort \
    --set ui.service.type=NodePort \
    --wait --timeout 300s

# The tags stay :latest with pullPolicy Never, so the upgrade alone leaves the
# old pods on the previous image; restart them onto the freshly imported one.
kubectl -n "\$NS" rollout restart deployment,daemonset
kubectl -n "\$NS" rollout status deployment --timeout=300s

echo ""
echo "  === Paqtra pods ==="
kubectl -n "\$NS" get pods,svc
echo ""
API_PORT=\$(kubectl -n "\$NS" get svc paqtra-api -o jsonpath='{.spec.ports[0].nodePort}' 2>/dev/null || true)
UI_PORT=\$(kubectl -n "\$NS" get svc paqtra-ui -o jsonpath='{.spec.ports[0].nodePort}' 2>/dev/null || true)
HOST_IP=\$(hostname -I | awk '{print \$1}')
UI_URL="https://\${HOST_IP}:\${UI_PORT}"
echo ""
echo "  =============================================="
echo "   Paqtra console — sign in here"
echo "  =============================================="
echo "   URL:      \${UI_URL}"
echo "   Username: admin"
ADMIN_PASS=\$(kubectl -n "\$NS" get secret paqtra-secret -o jsonpath='{.data.ADMIN_PASSWORD}' 2>/dev/null | base64 -d 2>/dev/null || true)
if [ -n "\$ADMIN_PASS" ]; then
  echo "   Password: \${ADMIN_PASS}"
else
  echo "   Password: (from Helm secret ADMIN_PASSWORD — auto-generated if unset)"
fi
echo "  =============================================="
echo "   (self-signed TLS — accept the browser warning)"
echo "   API NodePort: http://\${HOST_IP}:\${API_PORT}"
echo ""
if [ -n "\$UI_PORT" ]; then
  curl -skf "https://127.0.0.1:\${UI_PORT}/" >/dev/null && echo "  UI HTTPS: ok" || echo "  UI HTTPS: not ready yet"
fi
if [ -n "\$API_PORT" ]; then
  curl -sf "http://127.0.0.1:\${API_PORT}/health" | head -c 400 || echo "  health not ready yet"
  echo ""
fi
echo "PAQTRA_URL=\${UI_URL}"
echo "done — open \${UI_URL} (credentials from secret ADMIN_PASSWORD)"
REMOTE
    ok "Kubernetes deployment complete on ${TARGET_HOST}"
    echo ""
    echo "  Login: https://<host>:<UI NodePort>"
    echo "  User:  admin"
    echo "  Pass:  from Kubernetes secret ADMIN_PASSWORD (auto-generated if unset)"
    echo ""
}

# ─── Uninstall ───────────────────────────────────────────────

do_uninstall() {
    info "Uninstalling Paqtra from ${TARGET_HOST}..."
    _ssh bash <<REMOTE
set -e
export KUBECONFIG="\${KUBECONFIG:-\$HOME/.kube/config}"
if [ ! -r "\$KUBECONFIG" ] && [ -f /etc/rancher/k3s/k3s.yaml ]; then
    mkdir -p "\$HOME/.kube"
    sudo cp /etc/rancher/k3s/k3s.yaml "\$HOME/.kube/config"
    sudo chown "\$(id -u):\$(id -g)" "\$HOME/.kube/config"
    export KUBECONFIG="\$HOME/.kube/config"
fi

helm uninstall paqtra -n paqtra 2>/dev/null || true
kubectl delete namespace paqtra --wait=false 2>/dev/null || true
helm uninstall paqtra -n cilium-system 2>/dev/null || true

# Legacy host cleanup
systemctl stop paqtra-api paqtra-ui hubble-port-forward 2>/dev/null || true
systemctl disable paqtra-api paqtra-ui hubble-port-forward 2>/dev/null || true
sudo rm -f /usr/lib/systemd/system/paqtra-api.service /usr/lib/systemd/system/paqtra-ui.service /usr/lib/systemd/system/hubble-port-forward.service
sudo rm -f /usr/local/bin/paqtra-api /usr/local/bin/paqtra
sudo rm -rf /var/lib/paqtra /etc/paqtra /var/log/paqtra
sudo systemctl daemon-reload 2>/dev/null || true
rm -rf ${REMOTE_DIR}

echo "  ✅ Paqtra uninstalled"
REMOTE
    ok "Uninstall complete on ${TARGET_HOST}"
}

# ─── Fleet deploy ────────────────────────────────────────────

deploy_fleet() {
    local hosts_file="$1"
    [ -f "$hosts_file" ] || { fail "File not found: $hosts_file"; exit 1; }

    local count=0
    while IFS=' ' read -r host user pass opts; do
        [ -z "$host" ] && continue
        [[ "$host" =~ ^# ]] && continue

        echo ""
        echo "🚀 ═══ Deploying to ${host} ═══"
        TARGET_HOST="$host"
        TARGET_USER="${user:-root}"
        TARGET_PASS="${pass:-}"

        if [[ "$opts" == *"--quick"* ]] || [ "$QUICK_MODE" = true ]; then
            check_connectivity && uninstall_old && sync_binaries && install_quick
        elif [[ "$opts" == *"--k3s"* ]] || [ "$K3S_MODE" = true ]; then
            check_connectivity && sync_files && deploy_k3s
        else
            check_connectivity && sync_files && uninstall_old && install_full
        fi

        count=$((count + 1))
    done < "$hosts_file"

    echo ""
    ok "Deployed to ${count} host(s)"
}

# ─── Verify ──────────────────────────────────────────────────

verify() {
    info "Verifying Kubernetes deployment on ${TARGET_HOST}..."
    _ssh bash <<'REMOTE'
export KUBECONFIG="${KUBECONFIG:-$HOME/.kube/config}"
if [ ! -r "$KUBECONFIG" ] && [ -f /etc/rancher/k3s/k3s.yaml ]; then
    mkdir -p "$HOME/.kube"
    sudo cp /etc/rancher/k3s/k3s.yaml "$HOME/.kube/config"
    sudo chown "$(id -u):$(id -g)" "$HOME/.kube/config"
    export KUBECONFIG="$HOME/.kube/config"
fi

echo ""
echo "=== Cluster ==="
kubectl get nodes

echo ""
echo "=== Cilium / Hubble ==="
kubectl -n kube-system get pods -l k8s-app=cilium
kubectl -n kube-system get pods -l k8s-app=hubble-relay

echo ""
echo "=== Paqtra ==="
kubectl -n paqtra get pods,svc 2>/dev/null || echo "  (namespace paqtra not found)"

API_PORT=$(kubectl -n paqtra get svc paqtra-api -o jsonpath='{.spec.ports[0].nodePort}' 2>/dev/null || true)
if [ -n "$API_PORT" ]; then
    echo ""
    echo "=== Health ==="
    curl -sf "http://127.0.0.1:${API_PORT}/health" | python3 -m json.tool 2>/dev/null         || curl -sf "http://127.0.0.1:${API_PORT}/health"         || echo "  API not responding yet"
fi
REMOTE
}

# ─── Main ────────────────────────────────────────────────────

main() {
    echo ""
    echo "🔷 ══════════════════════════════════════════════════"
    echo "🔷    Paqtra v${VERSION} — Remote Deploy"
    echo "🔷 ══════════════════════════════════════════════════"
    echo ""

    # Fleet mode
    if [ "${FLEET_FILE}" != "" ]; then
        deploy_fleet "${FLEET_FILE}"
        exit 0
    fi

    validate
    check_connectivity

    if [ "$UNINSTALL" = true ]; then
        do_uninstall
        exit 0
    fi

    sync_files
    deploy_kubernetes
    verify

    echo ""
    echo "✅ ══════════════════════════════════════════════════"
    echo "✅    Kubernetes deployment complete: ${TARGET_HOST}"
    echo "✅ ══════════════════════════════════════════════════"
    echo ""
    echo "  UI/API: kubectl -n paqtra get svc — NodePorts"
    echo "  Host:   ${TARGET_HOST}"
    echo ""
}

main
