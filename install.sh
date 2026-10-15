#!/bin/sh
# ─────────────────────────────────────────────────────────────
# Paqtra CLI installer (like cilium-cli's install snippet).
#
#   curl -fsSL https://raw.githubusercontent.com/zyvorai/paqtra/main/install.sh | sh
#   curl -fsSL .../install.sh | sh -s -- --version 2.2.3
#
# Downloads the release tarball for this OS/arch, verifies it against the
# release's sha256sums.txt (and its cosign bundle when `cosign` is installed),
# and installs the `paqtra` binary. Then: paqtra install
#
# Options:
#   --version X       install this release (default: latest; env PAQTRA_VERSION)
#   --install-dir D   where to put the binary (env PAQTRA_INSTALL_DIR)
#   --from-source     build from a source checkout instead (developers)
#   --uninstall       remove the installed binary
# Env: PAQTRA_RELEASE_URL overrides the download base (used by tests).
# ─────────────────────────────────────────────────────────────
set -eu

REPO="zyvorai/paqtra"
VERSION="${PAQTRA_VERSION:-}"
INSTALL_DIR="${PAQTRA_INSTALL_DIR:-}"
ACTION="install"

say()  { printf '%s\n' "$*"; }
info() { printf '→ %s\n' "$*"; }
ok()   { printf '✔ %s\n' "$*"; }
warn() { printf '⚠ %s\n' "$*" >&2; }
die()  { printf '✖ %s\n' "$*" >&2; exit 1; }

usage() {
    sed -n '2,19p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//' || true
    exit "${1:-0}"
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)     [ $# -ge 2 ] || die "--version needs a value"; VERSION="$2"; shift 2 ;;
        --version=*)   VERSION="${1#--version=}"; shift ;;
        --install-dir) [ $# -ge 2 ] || die "--install-dir needs a value"; INSTALL_DIR="$2"; shift 2 ;;
        --install-dir=*) INSTALL_DIR="${1#--install-dir=}"; shift ;;
        --from-source) ACTION="source"; shift ;;
        --uninstall)   ACTION="uninstall"; shift ;;
        -h|--help)     usage 0 ;;
        *)             warn "unknown option: $1"; usage 1 ;;
    esac
done

# 2.2.3 and v2.2.3 both work.
VERSION="${VERSION#v}"

need() { command -v "$1" >/dev/null 2>&1 || die "$1 is required"; }

detect_os() {
    case "$(uname -s)" in
        Linux)  echo linux ;;
        Darwin) echo macos ;;
        *)      die "unsupported OS: $(uname -s) (Linux and macOS only)" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  echo x86_64 ;;
        aarch64|arm64) echo aarch64 ;;
        *)             die "unsupported architecture: $(uname -m)" ;;
    esac
}

# Where the binary goes: a writable system dir, else /usr/local/bin via sudo,
# else ~/.local/bin.
pick_install_dir() {
    if [ -n "$INSTALL_DIR" ]; then echo "$INSTALL_DIR"; return; fi
    if [ -w /usr/local/bin ]; then echo /usr/local/bin; return; fi
    if command -v sudo >/dev/null 2>&1 && [ "$(id -u)" != 0 ]; then echo /usr/local/bin; return; fi
    echo "${HOME}/.local/bin"
}

# Run a command as root only if the destination needs it.
maybe_sudo() {
    dir="$1"; shift
    if [ -w "$dir" ] || { [ ! -e "$dir" ] && [ -w "$(dirname "$dir")" ]; }; then
        "$@"
    elif command -v sudo >/dev/null 2>&1; then
        sudo "$@"
    else
        die "cannot write to $dir and sudo is not available (use --install-dir)"
    fi
}

latest_version() {
    # The /releases/latest redirect avoids the API's rate limit and needs no jq.
    url=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest") \
        || die "could not resolve the latest release (pass --version)"
    tag="${url##*/}"
    case "$tag" in
        v[0-9]*) echo "${tag#v}" ;;
        *) die "no published release found (got '$url'); pass --version" ;;
    esac
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print $1}'
    else die "sha256sum or shasum is required to verify the download"
    fi
}

do_source() {
    dir=$(cd "$(dirname "$0")" && pwd)
    [ -f "$dir/scripts/dev-install.sh" ] || die "--from-source needs a source checkout (scripts/dev-install.sh not found)"
    exec sh -c "exec bash \"$dir/scripts/dev-install.sh\" install"
}

do_uninstall() {
    dir=$(pick_install_dir)
    bin="$dir/paqtra"
    if [ ! -e "$bin" ]; then say "nothing to remove at $bin"; exit 0; fi
    maybe_sudo "$dir" rm -f "$bin"
    ok "removed $bin"
    say "The cluster release is separate: paqtra uninstall (run it before removing the binary)."
}

do_install() {
    need curl
    need tar
    os=$(detect_os)
    arch=$(detect_arch)
    [ -n "$VERSION" ] || { info "resolving latest release..."; VERSION=$(latest_version); }

    asset="paqtra-${VERSION}-${os}-${arch}.tar.gz"
    base="${PAQTRA_RELEASE_URL:-https://github.com/${REPO}/releases/download/v${VERSION}}"

    tmp=$(mktemp -d 2>/dev/null || mktemp -d -t paqtra)
    trap 'rm -rf "$tmp"' EXIT INT TERM

    info "downloading paqtra ${VERSION} (${os}/${arch})..."
    curl -fsSL -o "$tmp/$asset" "$base/$asset" \
        || die "download failed: $base/$asset (does v${VERSION} exist for ${os}/${arch}?)"
    curl -fsSL -o "$tmp/sha256sums.txt" "$base/sha256sums.txt" \
        || die "download failed: $base/sha256sums.txt (refusing to install an unverified binary)"

    info "verifying checksum..."
    want=$(awk -v f="$asset" '$2 == f || $2 == "*" f {print $1}' "$tmp/sha256sums.txt")
    [ -n "$want" ] || die "$asset is not listed in sha256sums.txt"
    got=$(sha256_of "$tmp/$asset")
    [ "$want" = "$got" ] || die "checksum mismatch for $asset (expected $want, got $got); nothing was installed"
    ok "sha256 ok"

    # Signature: verified whenever cosign is available; without it the checksum
    # (which the signature covers) is the guarantee, and we say so.
    if command -v cosign >/dev/null 2>&1; then
        if curl -fsSL -o "$tmp/sha256sums.txt.sigstore.json" "$base/sha256sums.txt.sigstore.json" 2>/dev/null; then
            info "verifying cosign signature..."
            cosign verify-blob \
                --bundle "$tmp/sha256sums.txt.sigstore.json" \
                --certificate-identity-regexp "^https://github.com/${REPO}/\.github/workflows/release\.yml@refs/tags/v" \
                --certificate-oidc-issuer https://token.actions.githubusercontent.com \
                "$tmp/sha256sums.txt" >/dev/null 2>&1 \
                || die "cosign signature verification FAILED; nothing was installed"
            ok "signature ok"
        else
            warn "no cosign bundle published for this release; skipped signature check"
        fi
    else
        info "cosign not found; skipped signature check (install cosign to verify releases)"
    fi

    tar -xzf "$tmp/$asset" -C "$tmp" paqtra || die "could not extract paqtra from $asset"
    [ -x "$tmp/paqtra" ] || chmod +x "$tmp/paqtra"

    dir=$(pick_install_dir)
    maybe_sudo "$dir" mkdir -p "$dir"
    maybe_sudo "$dir" install -m 755 "$tmp/paqtra" "$dir/paqtra"
    ok "installed $dir/paqtra"

    case ":$PATH:" in
        *":$dir:"*) ;;
        *) warn "$dir is not on your PATH; add: export PATH=\"$dir:\$PATH\"" ;;
    esac

    say ""
    "$dir/paqtra" version --client 2>/dev/null || "$dir/paqtra" version 2>/dev/null || true
    say ""
    say "Next:"
    say "  paqtra install         # install Paqtra into the current kube-context"
    say "  paqtra status --wait"
    say "  paqtra doctor          # check Cilium/Hubble prerequisites"
}

case "$ACTION" in
    install)   do_install ;;
    uninstall) do_uninstall ;;
    source)    do_source ;;
esac
