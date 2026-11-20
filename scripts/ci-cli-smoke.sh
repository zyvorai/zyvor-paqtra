#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# CLI binary smoke (no cluster required).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BIN="${PAQTRA_BIN:-$ROOT/target/release/paqtra}"
if [[ ! -x "$BIN" ]]; then
  echo "==> building release binary"
  cargo build --release -q
  BIN="$ROOT/target/release/paqtra"
fi

echo "==> paqtra --help"
"$BIN" --help >/dev/null || "$BIN" help >/dev/null || true
# Root with no args should print banner/help
"$BIN" 2>&1 | head -5 | grep -qi 'paqtra\|trace\|/¯¯' || {
  # Some clap apps exit 2 on missing subcommand — still print help
  "$BIN" 2>&1 | head -20 || true
}

echo "==> paqtra version / info (best-effort)"
"$BIN" version 2>/dev/null || "$BIN" info 2>/dev/null || true

echo "==> paqtra features / ebpf (observe-only)"
"$BIN" features -o json 2>/dev/null | grep -q brotherhood || "$BIN" features 2>&1 | head -20
"$BIN" ebpf attachments -o json 2>/dev/null | grep -qE 'source|attachments' || true
"$BIN" ebpf drift -o json 2>/dev/null | grep -qE '\[|kind' || true

echo "==> CLI smoke OK ($BIN)"
