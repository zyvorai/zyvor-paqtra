#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

# ─────────────────────────────────────────────────────────────
# Paqtra — Hyper SDK Cloud Deployment
# Deploy to Hyper.sh / Hyper_ container cloud platform
# ─────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Configuration
HYPER_REGION="${HYPER_REGION:-us-west-1}"
HYPER_SIZE="${HYPER_SIZE:-s4}"
IMAGE_REGISTRY="${IMAGE_REGISTRY:-ghcr.io/zyvorai}"
API_IMAGE="${IMAGE_REGISTRY}/paqtra-api:latest"
UI_IMAGE="${IMAGE_REGISTRY}/paqtra-ui:latest"
COMBINED_IMAGE="${IMAGE_REGISTRY}/paqtra:latest"
API_PORT="${API_PORT:-9191}"
UI_PORT="${UI_PORT:-8080}"
HYPER_API_NAME="paqtra-api"
HYPER_UI_NAME="paqtra-ui"
HYPER_FIP="${HYPER_FIP:-}"

GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo "🔷 [INFO] $*"; }
ok()    { echo "✅ [OK]   $*"; }
warn()  { echo "⚠️  $*"; }
err()   { echo "❌ [ERR]  $*" >&2; }

# ─── Check Hyper CLI ─────────────────────────────────────────
check_hyper() {
    if ! command -v hyper &>/dev/null; then
        err "Hyper CLI not found."
        echo ""
        echo "Install Hyper CLI:"
        echo "  curl -sSL https://hyper.sh/install | bash"
        echo ""
        echo "Configure credentials:"
        echo "  hyper config --accesskey <key> --secretkey <secret>"
        echo "  hyper config --default-region ${HYPER_REGION}"
        exit 1
    fi
    ok "Hyper CLI found"
}

# ─── Build & Push ────────────────────────────────────────────
build_and_push() {
    info "Building and pushing images..."

    local runtime="docker"
    command -v podman &>/dev/null && runtime="podman"

    # Build combined image
    info "Building combined image: ${COMBINED_IMAGE}"
    ${runtime} build -t "${COMBINED_IMAGE}" -f "${PROJECT_DIR}/Dockerfile.combined" "${PROJECT_DIR}/"

    # Push
    info "Pushing to registry..."
    ${runtime} push "${COMBINED_IMAGE}"

    ok "Images pushed to ${IMAGE_REGISTRY}"
}

# ─── Deploy API ──────────────────────────────────────────────
deploy_api() {
    local jwt_secret
    jwt_secret=$(openssl rand -hex 32)

    info "Deploying API server..."

    # Remove existing
    hyper rm -f "${HYPER_API_NAME}" 2>/dev/null || true

    hyper run -d \
        --name "${HYPER_API_NAME}" \
        --size "${HYPER_SIZE}" \
        --restart=always \
        -e "PAQTRA_HOST=0.0.0.0" \
        -e "PAQTRA_PORT=${API_PORT}" \
        -e "JWT_SECRET=${jwt_secret}" \
        -e "RUST_LOG=info" \
        -e "ALLOWED_ORIGINS=http://${HYPER_FIP:-localhost}:${UI_PORT}" \
        -p "${API_PORT}:${API_PORT}" \
        "${COMBINED_IMAGE}"

    ok "API deployed on port ${API_PORT}"
}

# ─── Deploy UI ───────────────────────────────────────────────
deploy_ui() {
    info "Deploying UI..."

    # Remove existing
    hyper rm -f "${HYPER_UI_NAME}" 2>/dev/null || true

    hyper run -d \
        --name "${HYPER_UI_NAME}" \
        --size s1 \
        --restart=always \
        --link "${HYPER_API_NAME}:paqtra-api" \
        -p "${UI_PORT}:8080" \
        "${UI_IMAGE}"

    ok "UI deployed on port ${UI_PORT}"
}

# ─── Floating IP ─────────────────────────────────────────────
attach_fip() {
    if [ -n "${HYPER_FIP}" ]; then
        info "Attaching floating IP: ${HYPER_FIP}"
        hyper fip attach "${HYPER_FIP}" "${HYPER_API_NAME}"
        ok "FIP attached: ${HYPER_FIP}:${API_PORT}"
    else
        info "Allocating new floating IP..."
        local fip
        fip=$(hyper fip allocate 1 2>/dev/null | tail -1)
        if [ -n "$fip" ]; then
            hyper fip attach "$fip" "${HYPER_API_NAME}"
            HYPER_FIP="$fip"
            ok "FIP allocated and attached: ${fip}:${API_PORT}"
        else
            warn "Could not allocate FIP. Use 'hyper fip allocate' manually."
        fi
    fi
}

# ─── Full Deploy ─────────────────────────────────────────────
full_deploy() {
    echo ""
    echo "🔷 ═══════════════════════════════════════════════════"
    echo "🔷    Paqtra — Hyper SDK Deployment"
    echo "🔷 ═══════════════════════════════════════════════════"
    echo ""

    check_hyper
    deploy_api
    deploy_ui
    attach_fip

    echo ""
    show_status
}

# ─── Status ──────────────────────────────────────────────────
show_status() {
    echo "🔷 ═══ Deployment Status ═══"
    echo ""
    hyper ps -a --filter "name=paqtra" 2>/dev/null || echo "  No containers found"
    echo ""

    if [ -n "${HYPER_FIP}" ]; then
        echo -e "  ${GREEN}Access:"
        echo -e "    API: http://${HYPER_FIP}:${API_PORT}"
        echo -e "    UI:  http://${HYPER_FIP}:${UI_PORT}"
    fi
    echo ""
}

# ─── Teardown ────────────────────────────────────────────────
teardown() {
    warn "Tearing down Hyper deployment..."
    hyper rm -f "${HYPER_UI_NAME}" 2>/dev/null || true
    hyper rm -f "${HYPER_API_NAME}" 2>/dev/null || true

    if [ -n "${HYPER_FIP}" ]; then
        hyper fip detach "${HYPER_API_NAME}" 2>/dev/null || true
        hyper fip release "${HYPER_FIP}" 2>/dev/null || true
    fi

    ok "Hyper deployment removed"
}

# ─── Logs ────────────────────────────────────────────────────
show_logs() {
    local container="${1:-${HYPER_API_NAME}}"
    hyper logs --tail 100 -f "${container}"
}

# ─── Compose Deploy (alternative) ───────────────────────────
compose_deploy() {
    info "Deploying via Hyper Compose..."

    cat > /tmp/hyper-compose.yml <<EOF
version: '2'
services:
  api:
    image: ${COMBINED_IMAGE}
    container_name: ${HYPER_API_NAME}
    size: ${HYPER_SIZE}
    environment:
      PAQTRA_HOST: "0.0.0.0"
      PAQTRA_PORT: "${API_PORT}"
      JWT_SECRET: "$(openssl rand -hex 32)"
      RUST_LOG: "info"
      ALLOWED_ORIGINS: "http://${HYPER_FIP:-localhost}:${UI_PORT}"
    ports:
      - "${API_PORT}:${API_PORT}"

  ui:
    image: ${UI_IMAGE}
    container_name: ${HYPER_UI_NAME}
    size: s1
    links:
      - api:paqtra-api
    ports:
      - "${UI_PORT}:8080"
EOF

    hyper compose up -f /tmp/hyper-compose.yml -d
    rm -f /tmp/hyper-compose.yml
    ok "Hyper Compose deployment complete"
}

# ─── Usage ───────────────────────────────────────────────────
usage() {
    cat <<EOF
Paqtra — Hyper SDK Cloud Deployment

Usage: $0 <command>

Commands:
  deploy          Full deployment (API + UI + FIP)
  build-push      Build and push images to registry
  compose         Deploy via Hyper Compose
  status          Show deployment status
  logs [name]     Stream container logs
  teardown        Remove all Hyper containers and FIPs

Environment:
  HYPER_REGION       Hyper region (default: us-west-1)
  HYPER_SIZE         Container size (default: s4)
  HYPER_FIP          Existing floating IP to attach
  IMAGE_REGISTRY     Image registry (default: ghcr.io/zyvorai)
  API_PORT           API port (default: 9191)
  UI_PORT            UI port (default: 8080)

Container sizes: s1 (64MB), s2 (128MB), s3 (256MB), s4 (512MB),
                 s6 (1GB), s8 (2GB), s12 (4GB), s16 (8GB)

Prerequisites:
  - Hyper CLI installed: curl -sSL https://hyper.sh/install | bash
  - Hyper credentials configured: hyper config
  - Images pushed to registry: $0 build-push

EOF
}

# ─── Main ────────────────────────────────────────────────────
case "${1:-help}" in
    deploy)      full_deploy ;;
    build-push)  build_and_push ;;
    compose)     check_hyper; compose_deploy ;;
    status)      show_status ;;
    logs)        show_logs "${2:-}" ;;
    teardown)    teardown ;;
    help|*)      usage ;;
esac
