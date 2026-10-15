#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
# Paqtra — DEVELOPER installer: builds the CLI from a source checkout.
# End users: use ./install.sh (downloads and verifies a release).
# No systemd. Cluster install is: paqtra install
# ─────────────────────────────────────────────────────────────
set -euo pipefail

VERSION="2.2.3"
INSTALL_DIR="/usr/local/bin"
SHARE_DIR="/usr/share/paqtra"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "${SCRIPT_DIR}" && pwd)"
# When invoked as install.sh from repo root
if [ -f "${SCRIPT_DIR}/Cargo.toml" ]; then
    PROJECT_DIR="${SCRIPT_DIR}"
elif [ -f "${SCRIPT_DIR}/../Cargo.toml" ]; then
    PROJECT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
fi

GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${BLUE}→${NC} $*"; }
ok()    { echo -e "${GREEN}✔${NC} $*"; }
warn()  { echo -e "${YELLOW}⚠${NC} $*"; }
err()   { echo -e "${RED}✖${NC} $*" >&2; }
die()   { err "$*"; exit 1; }

print_banner() {
    echo ""
    echo -e "${YELLOW}    /¯¯\\\\${NC}"
    echo -e "${BLUE} /¯¯${YELLOW}\\\\__/${GREEN}¯¯\\\\${NC}"
    echo -e "${BLUE} \\\\__${RED}/¯¯\\\\${GREEN}__/${NC}"
    echo -e "${GREEN} /¯¯${RED}\\\\__/${MAGENTA:-\\033[0;35m}¯¯\\\\${NC}"
    echo -e "${GREEN} \\\\__${BLUE}/¯¯\\\\${MAGENTA:-\\033[0;35m}__/${NC}"
    echo -e "${BLUE}    \\\\__/${NC}"
    echo ""
    echo "  Paqtra CLI installer v${VERSION}"
    echo ""
}

remove_legacy_systemd() {
    info "Removing legacy host systemd units (if any)..."
    sudo systemctl stop paqtra-api paqtra-ui hubble-port-forward 2>/dev/null || true
    sudo systemctl disable paqtra-api paqtra-ui hubble-port-forward 2>/dev/null || true
    sudo rm -f /usr/lib/systemd/system/paqtra-api.service \
               /usr/lib/systemd/system/paqtra-ui.service \
               /usr/lib/systemd/system/hubble-port-forward.service
    sudo rm -f /usr/local/bin/paqtra-api
    sudo systemctl daemon-reload 2>/dev/null || true
    ok "Legacy systemd cleaned"
}

build_cli() {
    info "Building paqtra CLI..."
    command -v cargo >/dev/null || die "Rust/cargo required. Install: https://rustup.rs"
    cd "${PROJECT_DIR}"
    cargo build --release
    ok "Built target/release/paqtra"
}

install_cli() {
    local bin="${PROJECT_DIR}/target/release/paqtra"
    [ -x "$bin" ] || die "Binary not found at $bin — run build first"
    info "Installing CLI to ${INSTALL_DIR}/paqtra"
    sudo install -m 755 "$bin" "${INSTALL_DIR}/paqtra"

    info "Installing Helm chart to ${SHARE_DIR}/chart"
    sudo mkdir -p "${SHARE_DIR}"
    sudo rm -rf "${SHARE_DIR}/chart"
    sudo cp -a "${PROJECT_DIR}/chart" "${SHARE_DIR}/chart"

    ok "paqtra $(paqtra version 2>/dev/null || echo installed)"
    echo ""
    echo "  Next steps (Kubernetes cluster required):"
    echo "    export PAQTRA_CHART_DIR=${SHARE_DIR}/chart"
    echo "    paqtra install"
    echo "    paqtra status"
    echo ""
}

uninstall_cli() {
    remove_legacy_systemd
    sudo rm -f "${INSTALL_DIR}/paqtra"
    sudo rm -rf "${SHARE_DIR}"
    ok "Paqtra CLI removed from host"
    echo "  To remove the cluster release: paqtra uninstall"
}

usage() {
    print_banner
    cat <<EOF
Usage: $0 <command>

Commands:
  install     Build from source and install the paqtra CLI + chart (no systemd)
  build       Build release binary only
  uninstall   Remove CLI from host and legacy systemd units
  help        Show this help

Cluster lifecycle is managed by the CLI:
  paqtra install | status | info | upgrade | uninstall | tui

EOF
}

main() {
    case "${1:-help}" in
        install)
            print_banner
            remove_legacy_systemd
            build_cli
            install_cli
            ;;
        build)
            build_cli
            ;;
        uninstall)
            uninstall_cli
            ;;
        help|--help|-h) usage ;;
        *) usage; exit 1 ;;
    esac
}

main "$@"
