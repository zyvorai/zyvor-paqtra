#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Tests install.sh against a fake local "release" (file:// URL): it must install
# a correct download and refuse a tampered one, an unlisted asset, and a
# release with no checksums file. No network, no real binary needed.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/rel" "$T/bin" "$T/src"

case "$(uname -s)" in Linux) OS=linux ;; Darwin) OS=macos ;; *) echo "unsupported OS"; exit 1 ;; esac
case "$(uname -m)" in x86_64|amd64) ARCH=x86_64 ;; aarch64|arm64) ARCH=aarch64 ;; *) echo "unsupported arch"; exit 1 ;; esac
ASSET="paqtra-9.9.9-${OS}-${ARCH}.tar.gz"

sha() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }
fails=0
pass() { echo "PASS  $1"; }
fail() { echo "FAIL  $1"; fails=$((fails + 1)); }
install_sh() { PAQTRA_RELEASE_URL="file://$T/rel" sh "$ROOT/install.sh" --version "$1" --install-dir "$T/bin" >"$T/out" 2>&1; }

printf '#!/bin/sh\necho "paqtra 9.9.9"\n' > "$T/src/paqtra"; chmod +x "$T/src/paqtra"
tar -czf "$T/rel/$ASSET" -C "$T/src" paqtra
(cd "$T/rel" && sha "$ASSET" > sha256sums.txt)

# 1. a good release installs, with or without the v prefix
if install_sh v9.9.9 && [ -x "$T/bin/paqtra" ] && [ "$("$T/bin/paqtra")" = "paqtra 9.9.9" ]; then pass "installs a verified release"; else fail "installs a verified release"; cat "$T/out"; fi
rm -f "$T/bin/paqtra"

# 2. tampered tarball: refused, nothing installed
cp "$T/rel/$ASSET" "$T/good.tgz"; echo tampered >> "$T/rel/$ASSET"
if ! install_sh 9.9.9 && [ ! -e "$T/bin/paqtra" ] && grep -q "checksum mismatch" "$T/out"; then pass "refuses a tampered tarball"; else fail "refuses a tampered tarball"; cat "$T/out"; fi
cp "$T/good.tgz" "$T/rel/$ASSET"

# 3. asset missing from sha256sums.txt
echo "deadbeef  other.tar.gz" > "$T/rel/sha256sums.txt"
if ! install_sh 9.9.9 && [ ! -e "$T/bin/paqtra" ] && grep -q "not listed" "$T/out"; then pass "refuses an unlisted asset"; else fail "refuses an unlisted asset"; cat "$T/out"; fi

# 4. no checksums file at all
rm -f "$T/rel/sha256sums.txt"
if ! install_sh 9.9.9 && [ ! -e "$T/bin/paqtra" ] && grep -q "unverified" "$T/out"; then pass "refuses a release without checksums"; else fail "refuses a release without checksums"; cat "$T/out"; fi

# 5. uninstall
cp "$T/src/paqtra" "$T/bin/paqtra"
if PAQTRA_INSTALL_DIR="$T/bin" sh "$ROOT/install.sh" --uninstall >/dev/null 2>&1 && [ ! -e "$T/bin/paqtra" ]; then pass "uninstall removes the binary"; else fail "uninstall removes the binary"; fi

# 6. unknown option is an error, not a silent install
if ! sh "$ROOT/install.sh" --bogus >/dev/null 2>&1; then pass "rejects unknown options"; else fail "rejects unknown options"; fi

if [ "$fails" -eq 0 ]; then echo "install.sh: all checks passed"; else echo "install.sh: $fails check(s) failed"; exit 1; fi
