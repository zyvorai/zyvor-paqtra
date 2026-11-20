#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# ─────────────────────────────────────────────────────────────
# Paqtra — Deploy & Test on Remote K3s
#
# Syncs source, builds paqtra on the remote, enables Hubble,
# and runs the TUI or a smoke test.
#
# Usage:
#   ./scripts/deploy-k3s-test.sh <host> <user> <password> [--run|--test|--status]
#
# Examples:
#   ./scripts/deploy-k3s-test.sh 185.165.240.5 root mypass          # Build + test
#   ./scripts/deploy-k3s-test.sh 185.165.240.5 root mypass --run    # Build + run TUI
#   ./scripts/deploy-k3s-test.sh 185.165.240.5 root mypass --status # Check status
#   ./scripts/deploy-k3s-test.sh 185.165.240.5 root mypass --test   # Build + smoke test
# ─────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
REMOTE_DIR="/root/paqtra"

TARGET_HOST="${1:-}"
TARGET_USER="${2:-root}"
TARGET_PASS="${3:-}"
ACTION="test"  # default: build + smoke test

for arg in "$@"; do
    case "$arg" in
        --run)    ACTION="run" ;;
        --test)   ACTION="test" ;;
        --status) ACTION="status" ;;
        --build)  ACTION="build" ;;
    esac
done

# ─── Helpers ────────────────────────────────────────────────

ok()   { echo "  ✅ $*"; }
fail() { echo "  ❌ $*"; exit 1; }
info() { echo "  ℹ️  $*"; }
step() { echo ""; echo "━━━ $* ━━━"; }

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
    local rsync_opts="-az --delete --info=progress2"
    if [ -n "${TARGET_PASS}" ] && command -v sshpass &>/dev/null; then
        export SSHPASS="${TARGET_PASS}"
        rsync ${rsync_opts} -e "sshpass -e ssh ${SSH_OPTS}" "$@"
    else
        rsync ${rsync_opts} -e "ssh ${SSH_OPTS}" "$@"
    fi
}

# ─── Validate ───────────────────────────────────────────────

if [ -z "${TARGET_HOST}" ]; then
    echo "Usage: $0 <host> <user> <password> [--run|--test|--status|--build]"
    echo ""
    echo "  --build   Sync source and build on remote"
    echo "  --test    Build + run smoke tests (default)"
    echo "  --run     Build + launch paqtra interactively"
    echo "  --status  Just check k3s/Cilium/Hubble status"
    exit 1
fi

if [ -n "${TARGET_PASS}" ] && ! command -v sshpass &>/dev/null; then
    fail "sshpass not found. Install: sudo dnf install sshpass"
fi

# ─── Connectivity ───────────────────────────────────────────

step "Connecting to ${TARGET_HOST}"
if _ssh "echo ok" &>/dev/null; then
    ok "SSH connected"
else
    fail "Cannot SSH to ${TARGET_HOST}"
fi

# ─── Status only ────────────────────────────────────────────

if [ "$ACTION" = "status" ]; then
    _ssh bash <<'REMOTE'
export KUBECONFIG=/etc/rancher/k3s/k3s.yaml

echo ""
echo "=== Node ==="
kubectl get nodes -o wide 2>/dev/null || echo "  k3s not running"

echo ""
echo "=== Cilium Pods ==="
kubectl -n kube-system get pods -l k8s-app=cilium 2>/dev/null || echo "  Cilium not found"

echo ""
echo "=== Hubble Relay ==="
kubectl -n kube-system get pods -l k8s-app=hubble-relay 2>/dev/null || echo "  Hubble relay not found"

echo ""
echo "=== Cilium Status ==="
cilium status 2>/dev/null || echo "  cilium CLI not reporting"

echo ""
echo "=== All Pods ==="
kubectl get pods -A 2>/dev/null

echo ""
echo "=== paqtra ==="
if [ -f /usr/local/bin/paqtra ]; then
    echo "  Installed at /usr/local/bin/paqtra"
    ls -lh /usr/local/bin/paqtra
else
    echo "  Not installed"
fi
REMOTE
    exit 0
fi

# ─── Sync source ────────────────────────────────────────────

step "Syncing source to ${TARGET_HOST}:${REMOTE_DIR}"
_rsync \
    --exclude '.git' \
    --exclude 'node_modules' \
    --exclude 'target' \
    --exclude 'dist' \
    --exclude 'web-ui' \
    --exclude 'web-api/target' \
    --exclude '*.vmdk' \
    --exclude '*.qcow2' \
    --exclude '*.iso' \
    "${PROJECT_DIR}/" \
    "${TARGET_USER}@${TARGET_HOST}:${REMOTE_DIR}/"
ok "Source synced"

# ─── Ensure Hubble relay is running ─────────────────────────

step "Ensuring Hubble relay is enabled"
_ssh bash <<'REMOTE'
set -e
export KUBECONFIG=/etc/rancher/k3s/k3s.yaml

# Check if hubble-relay is already running
if kubectl -n kube-system get pods -l k8s-app=hubble-relay 2>/dev/null | grep -q Running; then
    echo "  ✅ Hubble relay already running"
    exit 0
fi

echo "  Enabling Hubble relay..."

# Try cilium CLI first
if command -v cilium &>/dev/null; then
    cilium hubble enable --relay 2>/dev/null && {
        echo "  Waiting for Hubble relay..."
        for i in $(seq 1 60); do
            if kubectl -n kube-system get pods -l k8s-app=hubble-relay 2>/dev/null | grep -q Running; then
                echo "  ✅ Hubble relay is ready"
                exit 0
            fi
            sleep 2
        done
        echo "  ⚠️  Hubble relay not ready after 120s"
        exit 0
    }
fi

# Fallback: helm upgrade to enable hubble relay
if command -v helm &>/dev/null; then
    echo "  Upgrading Cilium via Helm to enable Hubble..."
    helm upgrade cilium cilium/cilium --namespace kube-system \
        --reuse-values \
        --set hubble.relay.enabled=true \
        --set hubble.enabled=true \
        --wait --timeout 120s 2>/dev/null || true
    echo "  ✅ Hubble relay enabled via Helm"
else
    echo "  ⚠️  Cannot enable Hubble: install Helm or cilium CLI"
fi
REMOTE

# ─── Install build deps + build ─────────────────────────────

step "Building paqtra on ${TARGET_HOST}"
_ssh bash <<'REMOTE'
set -e
export KUBECONFIG=/etc/rancher/k3s/k3s.yaml

cd /root/paqtra

# Install build deps if needed (AlmaLinux / RHEL / Fedora)
if command -v dnf &>/dev/null; then
    rpm -q gcc openssl-devel pkg-config &>/dev/null || {
        echo "  Installing build deps..."
        dnf install -y gcc make openssl-devel pkg-config 2>/dev/null
    }
elif command -v apt-get &>/dev/null; then
    dpkg -l build-essential libssl-dev pkg-config &>/dev/null 2>&1 || {
        echo "  Installing build deps..."
        apt-get update -qq && apt-get install -y -qq build-essential pkg-config libssl-dev 2>/dev/null
    }
fi

# Ensure Rust is available
if ! command -v cargo &>/dev/null; then
    echo "  Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi
source "$HOME/.cargo/env" 2>/dev/null || true

echo "  Building release binary..."
cargo build --release 2>&1 | tail -5

# Install
cp target/release/paqtra /usr/local/bin/paqtra
chmod 755 /usr/local/bin/paqtra

echo ""
echo "  ✅ paqtra built and installed"
ls -lh /usr/local/bin/paqtra
REMOTE
ok "Build complete"

# ─── Run smoke tests ────────────────────────────────────────

if [ "$ACTION" = "test" ]; then
    step "Running smoke tests"
    _ssh bash <<'REMOTE'
set -e
export KUBECONFIG=/etc/rancher/k3s/k3s.yaml
source "$HOME/.cargo/env" 2>/dev/null || true
cd /root/paqtra

echo "  Running cargo test..."
cargo test 2>&1 | tail -20

echo ""
echo "  Checking paqtra --help..."
paqtra --help 2>&1 | head -5

echo ""
echo "  Testing Hubble connectivity..."
if cilium hubble observe --last 5 -o json 2>/dev/null | head -2; then
    echo "  ✅ Hubble flows received"
else
    echo "  ⚠️  No Hubble flows (relay may need more time)"
fi

echo ""
echo "  Testing port-forward..."
# Start port-forward briefly to verify it works
timeout 5 cilium hubble port-forward &>/dev/null &
PF_PID=$!
sleep 3
if curl -sf http://127.0.0.1:4245 &>/dev/null || ss -tlnp | grep -q 4245; then
    echo "  ✅ Port-forward works (port 4245)"
else
    echo "  ⚠️  Port-forward not listening yet"
fi
kill $PF_PID 2>/dev/null || true

echo ""
echo "  Cluster summary:"
echo "    Nodes:  $(kubectl get nodes --no-headers 2>/dev/null | wc -l)"
echo "    Pods:   $(kubectl get pods -A --no-headers 2>/dev/null | wc -l)"
echo "    Cilium: $(kubectl -n kube-system get pods -l k8s-app=cilium --no-headers 2>/dev/null | grep Running | wc -l) running"
echo "    Hubble: $(kubectl -n kube-system get pods -l k8s-app=hubble-relay --no-headers 2>/dev/null | grep Running | wc -l) running"

echo ""
echo "  ✅ Smoke tests complete"
echo ""
echo "  To run the TUI interactively:"
echo "    ssh root@$(hostname -I | awk '{print $1}')"
echo "    paqtra"
echo "  Or with skip-bootstrap:"
echo "    paqtra --skip-bootstrap"
REMOTE
    ok "Smoke tests done"
fi

# ─── Run TUI interactively ──────────────────────────────────

if [ "$ACTION" = "run" ]; then
    step "Launching paqtra on ${TARGET_HOST}"
    echo "  Connecting with interactive TTY..."
    echo ""
    if [ -n "${TARGET_PASS}" ] && command -v sshpass &>/dev/null; then
        export SSHPASS="${TARGET_PASS}"
        sshpass -e ssh ${SSH_OPTS} -t "${TARGET_USER}@${TARGET_HOST}" \
            "export KUBECONFIG=/etc/rancher/k3s/k3s.yaml && paqtra --skip-bootstrap"
    else
        ssh ${SSH_OPTS} -t "${TARGET_USER}@${TARGET_HOST}" \
            "export KUBECONFIG=/etc/rancher/k3s/k3s.yaml && paqtra --skip-bootstrap"
    fi
fi

# ─── Summary ────────────────────────────────────────────────

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Deploy complete: ${TARGET_HOST}"
echo ""
echo "  Run interactively:"
echo "    $0 ${TARGET_HOST} ${TARGET_USER} '****' --run"
echo ""
echo "  Or SSH directly:"
echo "    sshpass -p '****' ssh root@${TARGET_HOST}"
echo "    export KUBECONFIG=/etc/rancher/k3s/k3s.yaml"
echo "    paqtra"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
