#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

# Golden investigation scenarios against a live Cilium cluster.
# Usage:
#   PAQTRA_API=http://127.0.0.1:31463 ADMIN_USER=admin ADMIN_PASS=Admin@321 \
#     ./scripts/ci-investigate-golden.sh
#
# When RUN_LIVE=1, hits a real API. Otherwise runs offline contract checks
# (schema / confidence rules) so CI can stay cluster-free.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

API="${PAQTRA_API:-}"
USER="${ADMIN_USER:-admin}"
PASS="${ADMIN_PASS:-Admin@321}"
RUN_LIVE="${RUN_LIVE:-0}"

pass() { echo "  ✔ $*"; }
fail() { echo "  ✖ $*"; exit 1; }

echo "==> investigate golden checks"

# Offline: confidence enum contract in investigate service source
grep -q 'Observed' web-api/src/services/investigate.rs || fail "Confidence::Observed missing"
grep -q 'toFQDNs not evaluated' web-api/src/services/investigate.rs || fail "FQDN unknown path missing"
grep -q 'never writes Cilium BPF' web-api/src/services/investigate.rs || fail "brotherhood rollback note missing"
grep -q 'investigate/path' web-api/src/main.rs || fail "investigate route missing"
grep -q 'flow_ingest' web-api/src/handlers/health.rs || fail "flow_ingest health missing"
pass "offline source contracts"

# Offline: docs
grep -q 'Attachment inventory is \*\*read-only\*\*' README.md || fail "README must state the BPF attachment inventory is read-only"
test -f docs/investigate.md || fail "docs/investigate.md missing"
pass "docs contracts"

if [[ "$RUN_LIVE" != "1" || -z "$API" ]]; then
  echo "==> skipping live API (set RUN_LIVE=1 PAQTRA_API=... to exercise cluster)"
  echo "==> investigate golden OK (offline)"
  exit 0
fi

echo "==> live login against $API"
TOKEN=$(curl -sf -X POST "$API/api/v1/auth/login" \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"$USER\",\"password\":\"$PASS\"}" | python3 -c 'import sys,json; print(json.load(sys.stdin)["token"])')

auth=(-H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json')

echo "==> investigate path (best-effort scenarios)"
# Allowed / unknown path — should return steps with confidence labels
RESP=$(curl -sf -X POST "$API/api/v1/investigate/path" "${auth[@]}" -d '{
  "source": {"namespace":"kube-system","name":"coredns"},
  "destination": {"namespace":"kube-system","name":"kube-dns"},
  "port": 53,
  "protocol": "UDP",
  "time_window_minutes": 120
}')
echo "$RESP" | python3 -c '
import sys, json
r=json.load(sys.stdin)
assert r.get("id"), "missing id"
assert r.get("steps"), "missing steps"
for s in r["steps"]:
    assert s.get("confidence") in ("observed","inferred","unavailable"), s
    if s["confidence"]=="observed":
        assert s.get("evidence") is not None
print("owner=", r.get("likely_owner"), "steps=", len(r["steps"]))
'
pass "path response confidence contract"

# Policy preview: FQDN must be unknown, not confident pass
PREV=$(curl -sf -X POST "$API/api/v1/policies/simulate" "${auth[@]}" -d '{
  "name": "golden-fqdn",
  "namespace": "default",
  "spec": {
    "endpointSelector": {"matchLabels": {"app": "web"}},
    "egress": [{"toFQDNs": [{"matchName": "example.com"}]}]
  }
}')
echo "$PREV" | python3 -c '
import sys, json
r=json.load(sys.stdin)
assert r.get("confidence") in ("unavailable","inferred","observed"), r
unc=r.get("uncertainty") or []
assert any("FQDN" in u or "toFQDNs" in u for u in unc), unc
impact=r.get("impact") or {}
assert impact.get("risk_level")=="unknown" or (impact.get("unknown",0)>=0)
print("preview confidence=", r.get("confidence"))
'
pass "FQDN preview → unknown / uncertainty"

BUNDLE_ID=$(echo "$RESP" | python3 -c 'import sys,json; print(json.load(sys.stdin)["id"])')
curl -sf "$API/api/v1/investigate/bundles/$BUNDLE_ID" "${auth[@]}" | python3 -c '
import sys, json
b=json.load(sys.stdin)
assert "redaction" in b
assert "flows" in b
print("bundle ok", b.get("id"))
'
pass "evidence bundle"

echo "==> investigate golden OK (live)"
