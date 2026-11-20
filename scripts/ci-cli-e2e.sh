#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# End-to-end test of the paqtra CLI against a real cluster (kind, no CNI).
# The CLI does everything, including installing Cilium, so a regression in any
# lifecycle command fails here. Expects images paqtra-api:ci, paqtra-ui:ci and
# paqtra:ci to be loaded into the cluster already.
#
#   PAQTRA=target/debug/paqtra scripts/ci-cli-e2e.sh
set -euo pipefail

PAQTRA="${PAQTRA:-target/debug/paqtra}"
NS=paqtra
export PAQTRA_NAMESPACE="$NS"

step() { printf '\n══ %s\n' "$*"; }
fail() { printf '✖ %s\n' "$*" >&2; exit 1; }

IMAGES=(
  --set api.image.repository=paqtra-api --set api.image.tag=ci --set api.image.pullPolicy=Never
  --set ui.image.repository=paqtra-ui --set ui.image.tag=ci --set ui.image.pullPolicy=Never
  --set agent.image.repository=paqtra --set agent.image.tag=ci --set agent.image.pullPolicy=Never
  --set api.persistence.enabled=false --set api.replicas=1 --set ui.replicas=1
  --set api.hpa.enabled=false --set monitoring.enabled=false
)

step "version (client only, no cluster needed)"
"$PAQTRA" version --client | tee /dev/stderr | grep -q "paqtra v"

step "status before install must fail (nothing there yet)"
if "$PAQTRA" status >/dev/null 2>&1; then fail "status succeeded with nothing installed"; fi

step "install --with-cilium (Cilium, Hubble, Relay, then Paqtra)"
"$PAQTRA" install --with-cilium --cilium-set operator.replicas=1 \
  --wait-duration 10m "${IMAGES[@]}"

step "status --wait"
"$PAQTRA" status --wait --wait-duration 5m

step "doctor must report no failures"
"$PAQTRA" doctor

step "version now sees the release and the server"
# The CLI colours its output; strip the escape codes before matching text.
out=$("$PAQTRA" version | sed 's/\x1b\[[0-9;]*m//g')
echo "$out"
grep -q "Release: .*chart paqtra-" <<<"$out" || fail "version did not report the release"
grep -q "Server: *Paqtra API v" <<<"$out" || fail "version did not reach the API"

step "config get/set round trip"
[ "$("$PAQTRA" config get api.env.hubbleMode)" = "grpc" ] || fail "unexpected hubbleMode"
"$PAQTRA" config set api.env.rustLog=debug
[ "$("$PAQTRA" config get api.env.rustLog)" = "debug" ] || fail "config set did not stick"

step "API token for the flows scenario"
kubectl -n "$NS" port-forward svc/paqtra-api 19191:9191 >/tmp/pf.log 2>&1 &
PF=$!
trap 'kill $PF 2>/dev/null || true' EXIT
ADMIN_PASSWORD=$(kubectl -n "$NS" get secret paqtra-secret -o jsonpath='{.data.ADMIN_PASSWORD}' | base64 -d)
[ -n "$ADMIN_PASSWORD" ] || fail "no admin password in the release secret"
for _ in $(seq 1 30); do curl -sf http://127.0.0.1:19191/health >/dev/null && break; sleep 2; done
PAQTRA_API_TOKEN=$(curl -sf http://127.0.0.1:19191/api/v1/auth/login \
  -H 'content-type: application/json' \
  -d "{\"username\":\"admin\",\"password\":\"$ADMIN_PASSWORD\"}" | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')
export PAQTRA_API_TOKEN

step "connectivity test (enforcement and flows, end to end)"
"$PAQTRA" connectivity test --timeout 4m

step "sysdump: complete, and free of the admin password"
rm -f /tmp/sysdump.zip
"$PAQTRA" sysdump -o /tmp/sysdump.zip --log-lines 200
unzip -l /tmp/sysdump.zip | grep -q "doctor.yaml" || fail "sysdump has no doctor report"
unzip -l /tmp/sysdump.zip | grep -q "helm/values.yaml" || fail "sysdump has no helm values"
if unzip -l /tmp/sysdump.zip | grep -qi "secret"; then fail "sysdump contains a file that looks like Secrets"; fi
if unzip -p /tmp/sysdump.zip | grep -qF "$ADMIN_PASSWORD"; then fail "the admin password leaked into the sysdump"; fi
if unzip -p /tmp/sysdump.zip | grep -qF "$PAQTRA_API_TOKEN"; then fail "the API token leaked into the sysdump"; fi

step "upgrade keeps values"
"$PAQTRA" upgrade --wait-duration 5m
[ "$("$PAQTRA" config get api.env.rustLog)" = "debug" ] || fail "upgrade lost the configured value"
"$PAQTRA" status --wait --wait-duration 5m

step "uninstall --purge"
kill "$PF" 2>/dev/null || true
"$PAQTRA" uninstall --purge --yes --wait
if "$PAQTRA" status >/dev/null 2>&1; then fail "status still succeeds after uninstall"; fi

printf '\n✔ CLI end-to-end test passed\n'
