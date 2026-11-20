#!/usr/bin/env bash
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

# Remote / local cluster smoke: pods Ready, API health, UI HTTPS, status CLI.
# Usage:
#   ./scripts/ci-remote-smoke.sh                 # local kubecontext, ns=paqtra
#   ./scripts/ci-remote-smoke.sh user@host       # ssh then check
# Env: PAQTRA_NAMESPACE (default paqtra), KUBECONFIG
set -euo pipefail

NS="${PAQTRA_NAMESPACE:-paqtra}"
TARGET="${1:-}"

remote_bash() {
  if [[ -z "$TARGET" ]]; then
    bash -c "$1"
  else
    ssh -o StrictHostKeyChecking=accept-new "$TARGET" "bash -lc $(printf %q "$1")"
  fi
}

echo "==> Paqtra remote smoke (ns=$NS${TARGET:+ target=$TARGET})"

remote_bash "
set -euo pipefail
export KUBECONFIG=\"\${KUBECONFIG:-\$HOME/.kube/config}\"
if [[ ! -r \"\$KUBECONFIG\" ]] && [[ -r /etc/rancher/k3s/k3s.yaml ]]; then
  export KUBECONFIG=\"\$HOME/.kube/paqtra-k3s.yaml\"
  mkdir -p \"\$HOME/.kube\"
  sudo cat /etc/rancher/k3s/k3s.yaml | sed \"s/127.0.0.1/\$(hostname -I | awk '{print \$1}')/\" > \"\$KUBECONFIG\"
  chmod 600 \"\$KUBECONFIG\"
fi

echo '-- pods --'
kubectl -n \"$NS\" get pods
kubectl -n \"$NS\" wait --for=condition=Ready pod -l app.kubernetes.io/instance=paqtra --timeout=120s 2>/dev/null \
  || kubectl -n \"$NS\" wait --for=condition=Ready pod --all --timeout=120s

API_PORT=\$(kubectl -n \"$NS\" get svc paqtra-api -o jsonpath='{.spec.ports[0].nodePort}' 2>/dev/null || true)
UI_PORT=\$(kubectl -n \"$NS\" get svc paqtra-ui -o jsonpath='{.spec.ports[0].nodePort}' 2>/dev/null || true)
HOST_IP=\$(hostname -I | awk '{print \$1}')

echo '-- API health --'
if [[ -n \"\$API_PORT\" ]]; then
  curl -sf \"http://127.0.0.1:\${API_PORT}/health\" | head -c 200
  echo
else
  POD=\$(kubectl -n \"$NS\" get pod -l app.kubernetes.io/component=api -o jsonpath='{.items[0].metadata.name}')
  kubectl -n \"$NS\" exec \"\$POD\" -- wget -qO- http://127.0.0.1:9191/health | head -c 200
  echo
fi

echo '-- UI HTTPS --'
if [[ -n \"\$UI_PORT\" ]]; then
  curl -skf \"https://127.0.0.1:\${UI_PORT}/\" >/dev/null && echo UI_OK
  echo \"PAQTRA_URL=https://\${HOST_IP}:\${UI_PORT}\"
  echo 'Login: admin / (Kubernetes secret ADMIN_PASSWORD)'
fi

echo '-- paqtra status (best-effort) --'
if command -v paqtra >/dev/null 2>&1; then
  paqtra status -n \"$NS\" || true
fi

echo 'remote smoke OK'
"
