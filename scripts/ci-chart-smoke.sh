#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

# Chart smoke: helm lint + template assertions (DaemonSet agent, BPF mount, TLS UI).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHART="${ROOT}/chart"

if ! command -v helm >/dev/null 2>&1; then
  echo "helm not installed — skip chart smoke" >&2
  exit 0
fi

echo "==> helm lint"
helm lint "$CHART" --set monitoring.enabled=false

echo "==> helm template (defaults)"
OUT="$(mktemp)"
helm template paqtra "$CHART" \
  --set monitoring.enabled=false \
  --set tls.enabled=true \
  >"$OUT"

grep -q 'kind: DaemonSet' "$OUT"
grep -q 'paqtra-agent\|component: agent' "$OUT"
grep -q '/sys/fs/bpf' "$OUT"
grep -q 'gen-cert\|tls.crt\|name: https' "$OUT"
grep -q 'containerPort: 8443\|UI_PORT' "$OUT" || grep -q '8443' "$OUT"

echo "==> helm template OK"
rm -f "$OUT"
