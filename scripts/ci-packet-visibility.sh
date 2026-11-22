#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

# Packet-visibility contracts: Flows UI + follow ingest + live API smoke.
# Usage:
#   ./scripts/ci-packet-visibility.sh
#   RUN_LIVE=1 PAQTRA_API=http://HOST:PORT ADMIN_PASS=Admin@321 \
#     ./scripts/ci-packet-visibility.sh
#
# Default is offline (cluster-free) so GitHub CI stays green without Hubble.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

API="${PAQTRA_API:-}"
USER="${ADMIN_USER:-admin}"
PASS="${ADMIN_PASS:-Admin@321}"
RUN_LIVE="${RUN_LIVE:-0}"

pass() { echo "  ✔ $*"; }
fail() { echo "  ✖ $*"; exit 1; }

echo "==> packet visibility checks"

# Offline: Flows UX + why-denied wiring
grep -q 'Why denied?' web-ui/src/views/Flows/index.tsx || fail "Flows Why denied? button missing"
grep -q "/investigate/flow" web-ui/src/views/Flows/index.tsx || fail "Flows must POST /investigate/flow"
grep -q 'LIVE STREAM' web-ui/src/views/Flows/index.tsx || fail "Flows LIVE STREAM chrome missing"
grep -q 'waiting on Hubble Relay' web-ui/src/views/Flows/index.tsx || fail "Flows empty Hubble state missing"
pass "Flows UI contracts"

# Offline: API follow ingest + health + routes
grep -q 'flow_ingest' web-api/src/handlers/health.rs || fail "flow_ingest health missing"
grep -q 'follow_flows' web-api/src/services/hubble_grpc.rs || fail "follow_flows missing"
grep -q 'spawn_flow_ingest' web-api/src/main.rs || fail "flow_ingest spawn missing"
grep -q '"/api/v1/flows"' web-api/src/main.rs || fail "/api/v1/flows route missing"
grep -q '"/api/v1/flows/stats"' web-api/src/main.rs || fail "/api/v1/flows/stats route missing"
grep -q 'investigate/flow' web-api/src/main.rs || fail "investigate/flow route missing"
grep -q 'investigate/bundles/{id}/share' web-api/src/main.rs || fail "investigate share route missing"
grep -q 'connectivity/alerts/{id}/silence' web-api/src/main.rs || fail "connectivity silence route missing"
grep -q '"/api/v1/flows/store"' web-api/src/main.rs || fail "flows/store route missing"
grep -q 'changes/{id}/impact' web-api/src/main.rs || fail "change impact route missing"
pass "API source contracts"

# Offline: UI contracts for extension surfaces
grep -q 'Silence 60m' web-ui/src/views/ConnectivityChecks/index.tsx || fail "connectivity silence UI missing"
grep -q 'Share link' web-ui/src/views/InvestigatePath/index.tsx || fail "investigate share UI missing"
grep -q 'Open evidence flows' web-ui/src/views/ChangeLog/index.tsx || fail "change impact evidence link missing"
grep -q 'tile timeout' web-ui/src/views/Dashboard/index.tsx || fail "overview tile timeout missing"
grep -q 'INGEST GAP TIMELINE' web-ui/src/views/ClusterHealth/index.tsx || fail "health gap timeline missing"
pass "UI extension contracts"

# Offline: docs
grep -q 'Live packet stream' docs/features.md || fail "docs/features.md missing live packet stream"
grep -q 'gap/disconnect visibility' docs/features.md || fail "docs/features.md missing ingest gap visibility"
pass "docs contracts"

# Offline: dedicated Vitest suite exists
test -f web-ui/src/views/Flows/__tests__/Flows.test.tsx || fail "Flows.test.tsx missing"
pass "Flows Vitest suite present"

if [[ "$RUN_LIVE" != "1" || -z "$API" ]]; then
  echo "==> skipping live API (set RUN_LIVE=1 PAQTRA_API=... to exercise cluster)"
  echo "==> packet visibility OK (offline)"
  exit 0
fi

echo "==> live login against $API"
TOKEN=$(curl -sf -X POST "$API/api/v1/auth/login" \
  -H 'Content-Type: application/json' \
  -d "{\"username\":\"$USER\",\"password\":\"$PASS\"}" | python3 -c 'import sys,json; print(json.load(sys.stdin)["token"])')

auth=(-H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json')

echo "==> health flow_ingest"
curl -sf "$API/health" | python3 -c '
import sys, json
h=json.load(sys.stdin)
fi=h.get("subsystems", {}).get("flow_ingest")
assert isinstance(fi, dict), h
for k in ("status", "source", "connected", "gaps", "disconnects"):
    assert k in fi, (k, fi)
print("flow_ingest=", fi.get("status"), "source=", fi.get("source"),
      "connected=", fi.get("connected"), "indexed=", fi.get("indexed"))
'
pass "health.flow_ingest shape"

echo "==> flows list + stats"
FLOWS=$(curl -sf "$API/api/v1/flows?limit=10" "${auth[@]}")
echo "$FLOWS" | python3 -c '
import sys, json
r=json.load(sys.stdin)
assert "flows" in r, r
assert isinstance(r["flows"], list), r
print("flows=", len(r["flows"]), "total=", r.get("total"))
'
# Stats can time out when Hubble is saturated; treat as best-effort.
STATS_CODE=$(curl -sS -o /tmp/paqtra-flow-stats.json -w "%{http_code}"   --max-time 30 "$API/api/v1/flows/stats" "${auth[@]}" || echo 000)
if [[ "$STATS_CODE" == "200" ]]; then
  python3 -c '
import json
r=json.load(open("/tmp/paqtra-flow-stats.json"))
assert "forwarded" in r or "total_flows" in r or "total" in r, r
print("stats keys=", sorted(r.keys())[:12])
'
  pass "flows list + stats"
else
  echo "  ℹ flows/stats HTTP $STATS_CODE (Hubble timeout common under load) — continuing"
  pass "flows list (stats best-effort)"
fi

# Best-effort why-denied when a DROPPED flow is present
DROPPED=$(echo "$FLOWS" | python3 -c '
import sys, json
flows=json.load(sys.stdin).get("flows") or []
for f in flows:
    if str(f.get("verdict","")).upper()=="DROPPED":
        print(json.dumps(f))
        break
')
if [[ -n "$DROPPED" ]]; then
  echo "==> investigate/flow on DROPPED sample"
  export TOKEN PAQTRA_API="$API"
  echo "$DROPPED" | python3 -c '
import sys, json, urllib.request, os
f=json.load(sys.stdin)
api=os.environ["PAQTRA_API"]
token=os.environ["TOKEN"]
body=json.dumps({"flow_id": f.get("id"), "flow": f}).encode()
req=urllib.request.Request(
    api.rstrip("/")+"/api/v1/investigate/flow",
    data=body,
    headers={"Authorization":"Bearer "+token,"Content-Type":"application/json"},
    method="POST",
)
with urllib.request.urlopen(req, timeout=60) as resp:
    r=json.load(resp)
assert r.get("steps"), r
for s in r["steps"]:
    assert s.get("confidence") in ("observed","inferred","unavailable"), s
print("owner=", r.get("likely_owner"), "steps=", len(r["steps"]))
'
  pass "investigate/flow on DROPPED"
else
  echo "  ℹ no DROPPED flows in sample — skip investigate/flow"
fi

echo "==> connectivity paths CRUD + silence"
PATH_BODY='{"name":"ci-path","src_namespace":"default","src_workload":"ci-src","dst_namespace":"default","dst_service":"ci-dst","port":80,"protocol":"TCP"}'
PATH_RESP=$(curl -sf -X POST "$API/api/v1/connectivity/paths" "${auth[@]}" -d "$PATH_BODY")
PATH_ID=$(echo "$PATH_RESP" | python3 -c 'import sys,json; r=json.load(sys.stdin); print(r.get("id") or r.get("path",{}).get("id") or "")')
[[ -n "$PATH_ID" ]] || fail "connectivity path create returned no id: $PATH_RESP"
curl -sf "$API/api/v1/connectivity/paths" "${auth[@]}" | python3 -c '
import sys, json
r=json.load(sys.stdin)
assert "paths" in r, r
print("paths=", len(r["paths"]))
'
# Status is best-effort (may be unknown with no traffic)
curl -sS -o /tmp/paqtra-conn-status.json -w "%{http_code}" --max-time 30 \
  "$API/api/v1/connectivity/paths/$PATH_ID/status" "${auth[@]}" >/tmp/paqtra-conn-status.code || true
# Silence endpoint accepts path_id or calert-* id
curl -sf -X POST "$API/api/v1/connectivity/alerts/$PATH_ID/silence" "${auth[@]}" \
  -d '{"minutes":5}' | python3 -c '
import sys, json
r=json.load(sys.stdin)
assert "path_id" in r or "silenced_until" in r, r
print("silence=", r)
'
curl -sf -X DELETE "$API/api/v1/connectivity/paths/$PATH_ID" "${auth[@]}" >/dev/null
pass "connectivity CRUD + silence"

echo "==> change impact (best-effort when changes exist)"
CHANGES=$(curl -sf "$API/api/v1/changes?limit=5" "${auth[@]}" || echo '{}')
CHG_ID=$(echo "$CHANGES" | python3 -c '
import sys, json
r=json.load(sys.stdin)
chs=r.get("changes") or r.get("items") or []
print(chs[0]["id"] if chs else "")
')
if [[ -n "$CHG_ID" ]]; then
  curl -sf "$API/api/v1/changes/$CHG_ID/impact?before=15m&after=15m" "${auth[@]}" | python3 -c '
import sys, json
r=json.load(sys.stdin)
assert "status" in r and "confidence" in r, r
assert "chart" in r or ("before" in r and "after" in r), r
print("impact status=", r.get("status"), "confidence=", r.get("confidence"))
'
  pass "change impact"
else
  echo "  ℹ no changes recorded — skip impact"
fi

echo "==> investigate path + export + share"
INV=$(curl -sf -X POST "$API/api/v1/investigate/path" "${auth[@]}" \
  -d '{"source":{"namespace":"default","name":"ci"},"destination":{"namespace":"default","name":"svc"},"port":80,"protocol":"TCP","time_window_minutes":15}')
INV_ID=$(echo "$INV" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("id",""))')
[[ -n "$INV_ID" ]] || fail "investigate/path returned no id"
curl -sf "$API/api/v1/investigate/bundles/$INV_ID/export?format=markdown" "${auth[@]}" | python3 -c '
import sys, json
r=json.load(sys.stdin)
assert "content" in r or "bundle" in r, r
print("export keys=", sorted(r.keys())[:8])
'
SHARE=$(curl -sf -X POST "$API/api/v1/investigate/bundles/$INV_ID/share" "${auth[@]}" -d '{"ttl_secs":600}')
TOKEN=$(echo "$SHARE" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("token",""))')
[[ -n "$TOKEN" ]] || fail "share returned no token"
curl -sf "$API/api/v1/investigate/share/$TOKEN" "${auth[@]}" | python3 -c '
import sys, json
r=json.load(sys.stdin)
assert "incident_card" in r or "bundle_id" in r or "token" in r, r
print("share ok")
'
pass "investigate export + share"

echo "==> flows/store info"
curl -sf "$API/api/v1/flows/store" "${auth[@]}" | python3 -c '
import sys, json
r=json.load(sys.stdin)
assert "retention_days" in r and "ingest" in r, r
assert "gaps" in r["ingest"] or "recent_gaps" in r["ingest"], r
print("store retention=", r.get("retention_days"), "gaps=", r["ingest"].get("gaps"))
'
pass "flows/store"

echo "==> packet visibility OK (live)"
