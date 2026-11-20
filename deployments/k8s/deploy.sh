#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# ─────────────────────────────────────────────────────────────
# Paqtra — Kubernetes Deployment Script
# Auto TLS, RBAC, Deployment, Service, Ingress, Health Check
# ─────────────────────────────────────────────────────────────
set -euo pipefail

NAMESPACE="${NAMESPACE:-cilium-system}"
IMAGE_REGISTRY="${IMAGE_REGISTRY:-ghcr.io/zyvorai}"
API_IMAGE="${IMAGE_REGISTRY}/paqtra-api:latest"
UI_IMAGE="${IMAGE_REGISTRY}/paqtra-ui:latest"
API_REPLICAS="${API_REPLICAS:-2}"
UI_REPLICAS="${UI_REPLICAS:-2}"
NODE_PORT="${NODE_PORT:-30919}"
TLS_ENABLED="${TLS_ENABLED:-false}"

GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo "🔷 [INFO] $*"; }
ok()    { echo "✅ [OK]   $*"; }
err()   { echo "❌ [ERR]  $*" >&2; }

# ─── Detect container runtime ───────────────────────────────
detect_runtime() {
    for rt in podman docker nerdctl; do
        if command -v "$rt" &>/dev/null; then
            RUNTIME="$rt"
            ok "Container runtime: $rt"
            return
        fi
    done
    err "No container runtime found (podman/docker/nerdctl)"
    exit 1
}

# ─── Build images ────────────────────────────────────────────
build_images() {
    detect_runtime
    info "Building API image: ${API_IMAGE}"
    ${RUNTIME} build -t "${API_IMAGE}" -f web-api/Dockerfile web-api/

    info "Building UI image: ${UI_IMAGE}"
    ${RUNTIME} build -t "${UI_IMAGE}" -f web-ui/Dockerfile web-ui/

    ok "Images built"
}

# ─── Push images ─────────────────────────────────────────────
push_images() {
    detect_runtime
    info "Pushing ${API_IMAGE}"
    ${RUNTIME} push "${API_IMAGE}"
    info "Pushing ${UI_IMAGE}"
    ${RUNTIME} push "${UI_IMAGE}"
    ok "Images pushed"
}

# ─── Generate TLS certificates ───────────────────────────────
generate_tls() {
    if [ "${TLS_ENABLED}" != "true" ]; then return; fi
    info "Generating self-signed TLS certificates..."
    local tmpdir
    tmpdir=$(mktemp -d)
    openssl req -x509 -newkey rsa:4096 -sha256 -days 365 -nodes \
        -keyout "${tmpdir}/tls.key" -out "${tmpdir}/tls.crt" \
        -subj "/CN=paqtra.${NAMESPACE}.svc" \
        -addext "subjectAltName=DNS:paqtra.${NAMESPACE}.svc,DNS:paqtra.${NAMESPACE}.svc.cluster.local,DNS:localhost" \
        2>/dev/null

    kubectl -n "${NAMESPACE}" create secret tls paqtra-tls \
        --cert="${tmpdir}/tls.crt" --key="${tmpdir}/tls.key" \
        --dry-run=client -o yaml | kubectl apply -f -

    rm -rf "${tmpdir}"
    ok "TLS secret created: paqtra-tls"
}

# ─── Generate JWT secret ────────────────────────────────────
generate_jwt_secret() {
    local secret
    secret=$(openssl rand -hex 32 2>/dev/null || head -c 64 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 64)
    kubectl -n "${NAMESPACE}" create secret generic paqtra-secrets \
        --from-literal=jwt-secret="${secret}" \
        --dry-run=client -o yaml | kubectl apply -f -
    ok "JWT secret created"
}

# ─── Deploy ──────────────────────────────────────────────────
deploy() {
    info "Deploying Paqtra to namespace: ${NAMESPACE}"

    # Create namespace
    kubectl create namespace "${NAMESPACE}" --dry-run=client -o yaml | kubectl apply -f -

    # Apply RBAC
    if [ -f deployments/k8s/rbac.yaml ]; then
        kubectl apply -f deployments/k8s/rbac.yaml
        ok "RBAC applied"
    fi

    # Apply ConfigMap
    if [ -f deployments/k8s/configmap.yaml ]; then
        kubectl apply -f deployments/k8s/configmap.yaml
        ok "ConfigMap applied"
    fi

    # Secrets
    generate_jwt_secret
    generate_tls

    # Apply deployments
    if [ -f deployments/k8s/backend-deployment.yaml ]; then
        kubectl apply -f deployments/k8s/backend-deployment.yaml
        ok "Backend deployment applied"
    fi

    if [ -f deployments/k8s/frontend-deployment.yaml ]; then
        kubectl apply -f deployments/k8s/frontend-deployment.yaml
        ok "Frontend deployment applied"
    fi

    # Apply ingress if exists
    if [ -f deployments/k8s/ingress.yaml ]; then
        kubectl apply -f deployments/k8s/ingress.yaml
        ok "Ingress applied"
    fi

    # Wait for rollout
    info "Waiting for deployments to be ready..."
    kubectl -n "${NAMESPACE}" rollout status deployment/paqtra-api --timeout=120s 2>/dev/null || true
    kubectl -n "${NAMESPACE}" rollout status deployment/paqtra-ui --timeout=120s 2>/dev/null || true

    # Health check
    info "Running health check..."
    local retries=15
    for i in $(seq 1 $retries); do
        local pod
        pod=$(kubectl -n "${NAMESPACE}" get pods -l app=paqtra-api -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || echo "")
        if [ -n "$pod" ]; then
            if kubectl -n "${NAMESPACE}" exec "$pod" -- wget -qO- http://localhost:9191/health 2>/dev/null | grep -q "ok\|healthy"; then
                ok "API health check passed"
                break
            fi
        fi
        [ "$i" -eq "$retries" ] && err "Health check failed after ${retries} attempts"
        sleep 4
    done

    show_status
}

# ─── Delete ──────────────────────────────────────────────────
delete_deployment() {
    info "Removing Paqtra from namespace: ${NAMESPACE}"
    kubectl delete -f deployments/k8s/ --ignore-not-found -n "${NAMESPACE}" 2>/dev/null || true
    kubectl delete secret paqtra-tls paqtra-secrets -n "${NAMESPACE}" --ignore-not-found 2>/dev/null || true
    ok "Deployment removed"
}

# ─── Status ──────────────────────────────────────────────────
show_status() {
    echo ""
    echo "🔷 ═══════════════════════════════════════════════════"
    echo "🔷    Paqtra — Kubernetes Deployment Status"
    echo "🔷 ═══════════════════════════════════════════════════"
    echo ""
    kubectl -n "${NAMESPACE}" get pods -l 'app in (paqtra-api,paqtra-ui)' -o wide 2>/dev/null || echo "  No pods found"
    echo ""
    kubectl -n "${NAMESPACE}" get svc -l 'app in (paqtra-api,paqtra-ui)' 2>/dev/null || echo "  No services found"
    echo ""

    # NodePort access
    local node_ip
    node_ip=$(kubectl get nodes -o jsonpath='{.items[0].status.addresses[?(@.type=="InternalIP")].address}' 2>/dev/null || echo "localhost")
    echo -e "  ${BLUE}Access:"
    echo -e "    API: http://${node_ip}:${NODE_PORT}"
    echo -e "    UI:  kubectl -n ${NAMESPACE} port-forward svc/paqtra-ui 3001:80"
    echo ""
}

# ─── Logs ────────────────────────────────────────────────────
show_logs() {
    local component="${1:-api}"
    kubectl -n "${NAMESPACE}" logs -l "app=paqtra-${component}" --tail=100 -f
}

# ─── Port Forward ────────────────────────────────────────────
port_forward() {
    info "Port-forwarding: API on :9191, UI on :3001"
    kubectl -n "${NAMESPACE}" port-forward svc/paqtra-api 9191:9191 &
    kubectl -n "${NAMESPACE}" port-forward svc/paqtra-ui 3001:80 &
    wait
}

# ─── Usage ───────────────────────────────────────────────────
usage() {
    cat <<EOF
Paqtra — Kubernetes Deployment

Usage: $0 <command>

Commands:
  build          Build container images
  push           Push images to registry
  deploy         Deploy to Kubernetes cluster
  delete         Remove deployment
  status         Show deployment status
  logs [api|ui]  Stream logs
  port-forward   Forward API and UI ports locally

Environment:
  NAMESPACE        K8s namespace (default: cilium-system)
  IMAGE_REGISTRY   Image registry (default: ghcr.io/zyvorai)
  API_REPLICAS     API pod replicas (default: 2)
  UI_REPLICAS      UI pod replicas (default: 2)
  TLS_ENABLED      Generate TLS certs (default: false)
  NODE_PORT        NodePort for API (default: 30919)

EOF
}

# ─── Main ────────────────────────────────────────────────────
cd "$(dirname "$0")/../.."

case "${1:-help}" in
    build)         build_images ;;
    push)          push_images ;;
    deploy)        deploy ;;
    delete)        delete_deployment ;;
    status)        show_status ;;
    logs)          show_logs "${2:-api}" ;;
    port-forward)  port_forward ;;
    help|--help)   usage ;;
    *)             err "Unknown: $1"; usage; exit 1 ;;
esac
