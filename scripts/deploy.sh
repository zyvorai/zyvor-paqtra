#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Paqtra - Build, Push & Deploy
# Usage:
#   ./scripts/deploy.sh build          Build Docker images locally
#   ./scripts/deploy.sh push           Push images to registry
#   ./scripts/deploy.sh up             Start with docker compose
#   ./scripts/deploy.sh down           Stop docker compose
#   ./scripts/deploy.sh k8s            Deploy to Kubernetes
#   ./scripts/deploy.sh k8s-delete     Remove from Kubernetes
#   ./scripts/deploy.sh all            Build + push + deploy to K8s
#   ./scripts/deploy.sh status         Show deployment status

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REGISTRY="${REGISTRY:-ghcr.io}"
REPO="${REPO:-zyvorai/paqtra}"
TAG="${TAG:-latest}"
K8S_NS="${K8S_NS:-cilium-system}"

API_IMAGE="${REGISTRY}/${REPO}-api:${TAG}"
UI_IMAGE="${REGISTRY}/${REPO}-ui:${TAG}"

red()    { printf "\033[31m%s\033[0m\n" "$*"; }
green()  { printf "\033[32m%s\033[0m\n" "$*"; }
blue()   { printf "\033[34m%s\033[0m\n" "$*"; }
bold()   { printf "\033[1m%s\033[0m\n" "$*"; }

# ─── Build ──────────────────────────────────────────────────────────────
cmd_build() {
    bold "Building Docker images..."
    blue "  API: ${API_IMAGE}"
    docker build --network host -t "${API_IMAGE}" "${ROOT}/web-api"
    green "  API image built"

    blue "  UI:  ${UI_IMAGE}"
    docker build --network host -t "${UI_IMAGE}" "${ROOT}/web-ui"
    green "  UI image built"

    echo ""
    green "Build complete. Images:"
    docker images --format "  {{.Repository}}:{{.Tag}}  {{.Size}}" | grep "${REPO}" || true
}

# ─── Push ───────────────────────────────────────────────────────────────
cmd_push() {
    bold "Pushing images to ${REGISTRY}..."
    docker push "${API_IMAGE}"
    green "  Pushed ${API_IMAGE}"
    docker push "${UI_IMAGE}"
    green "  Pushed ${UI_IMAGE}"
}

# ─── Docker Compose Up ──────────────────────────────────────────────────
cmd_up() {
    bold "Starting with docker compose..."

    # Ensure .env exists
    if [ ! -f "${ROOT}/deployments/.env" ]; then
        if [ -f "${ROOT}/deployments/.env.example" ]; then
            cp "${ROOT}/deployments/.env.example" "${ROOT}/deployments/.env"
            # Generate a random JWT secret
            JWT=$(openssl rand -base64 48)
            sed -i "s|your-secret-key-at-least-32-characters-long|${JWT}|" "${ROOT}/deployments/.env"
            green "  Generated .env with random JWT secret"
        else
            red "  No .env or .env.example found in deployments/"
            exit 1
        fi
    fi

    cd "${ROOT}/deployments"
    docker compose up --build -d
    echo ""
    green "Services started:"
    docker compose ps --format "  {{.Name}}  {{.Status}}  {{.Ports}}"
    echo ""
    blue "  UI:  http://localhost:3001"
    blue "  API: http://localhost:9191"
}

# ─── Docker Compose Down ────────────────────────────────────────────────
cmd_down() {
    bold "Stopping docker compose..."
    cd "${ROOT}/deployments"
    docker compose down
    green "  Stopped"
}

# ─── Kubernetes Deploy ──────────────────────────────────────────────────
cmd_k8s() {
    bold "Deploying to Kubernetes (namespace: ${K8S_NS})..."

    # Create namespace if missing
    kubectl create namespace "${K8S_NS}" --dry-run=client -o yaml | kubectl apply -f -

    # Generate JWT secret dynamically instead of applying hardcoded secrets.yaml
    local jwt_secret
    jwt_secret=$(openssl rand -base64 48)
    kubectl -n "${K8S_NS}" create secret generic paqtra-secrets \
        --from-literal=jwt-secret="${jwt_secret}" \
        --dry-run=client -o yaml | kubectl apply -f -
    green "  JWT secret generated and applied"

    # Apply manifests in order (skip secrets.yaml — generated above)
    local k8s_dir="${ROOT}/deployments/k8s"
    for f in configmap.yaml rbac.yaml backend-deployment.yaml frontend-deployment.yaml ingress.yaml; do
        if [ -f "${k8s_dir}/${f}" ]; then
            kubectl apply -n "${K8S_NS}" -f "${k8s_dir}/${f}"
            green "  Applied ${f}"
        fi
    done

    echo ""
    bold "Waiting for rollout..."
    kubectl -n "${K8S_NS}" rollout status deployment/paqtra-api --timeout=120s || true
    kubectl -n "${K8S_NS}" rollout status deployment/paqtra-ui --timeout=120s || true

    echo ""
    cmd_status
}

# ─── Kubernetes Delete ──────────────────────────────────────────────────
cmd_k8s_delete() {
    bold "Removing from Kubernetes (namespace: ${K8S_NS})..."
    kubectl delete namespace "${K8S_NS}" --ignore-not-found
    green "  Namespace ${K8S_NS} deleted"
}

# ─── Status ─────────────────────────────────────────────────────────────
cmd_status() {
    bold "Deployment Status"
    echo ""

    # Docker compose
    if docker compose -f "${ROOT}/deployments/docker-compose.yaml" ps --quiet 2>/dev/null | grep -q .; then
        blue "Docker Compose:"
        docker compose -f "${ROOT}/deployments/docker-compose.yaml" ps --format "  {{.Name}}  {{.Status}}"
        echo ""
    fi

    # Kubernetes
    if kubectl get namespace "${K8S_NS}" &>/dev/null; then
        blue "Kubernetes (${K8S_NS}):"
        kubectl -n "${K8S_NS}" get pods -o wide 2>/dev/null | sed 's/^/  /'
        echo ""
        kubectl -n "${K8S_NS}" get svc 2>/dev/null | sed 's/^/  /'
    else
        blue "Kubernetes: not deployed"
    fi
}

# ─── All ────────────────────────────────────────────────────────────────
cmd_all() {
    cmd_build
    echo ""
    cmd_push
    echo ""
    cmd_k8s
}

# ─── Main ───────────────────────────────────────────────────────────────
case "${1:-help}" in
    build)      cmd_build ;;
    push)       cmd_push ;;
    up)         cmd_up ;;
    down)       cmd_down ;;
    k8s)        cmd_k8s ;;
    k8s-delete) cmd_k8s_delete ;;
    all)        cmd_all ;;
    status)     cmd_status ;;
    *)
        bold "Paqtra Deploy Script"
        echo ""
        echo "Usage: $0 <command>"
        echo ""
        echo "Commands:"
        echo "  build       Build Docker images locally"
        echo "  push        Push images to registry"
        echo "  up          Start with docker compose (local dev)"
        echo "  down        Stop docker compose"
        echo "  k8s         Deploy to Kubernetes cluster"
        echo "  k8s-delete  Remove from Kubernetes"
        echo "  all         Build + push + deploy to K8s"
        echo "  status      Show deployment status"
        echo ""
        echo "Environment variables:"
        echo "  REGISTRY    Container registry (default: ghcr.io)"
        echo "  REPO        Repository name (default: zyvorai/paqtra)"
        echo "  TAG         Image tag (default: latest)"
        echo "  K8S_NS      Kubernetes namespace (default: cilium-system)"
        ;;
esac
