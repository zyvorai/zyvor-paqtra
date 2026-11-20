#!/bin/sh
# Copyright 2026 Zyvor AI Labs · https://zyvor.dev
# SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

set -eu

CERT_DIR="${TLS_CERT_DIR:-/certs}"
CERT="${TLS_CERT_PATH:-$CERT_DIR/tls.crt}"
KEY="${TLS_KEY_PATH:-$CERT_DIR/tls.key}"
LISTEN_PORT="${UI_PORT:-8443}"
API_UPSTREAM="${API_UPSTREAM:-paqtra-api:9191}"
API_SCHEME="${API_SCHEME:-http}"
TLS_DISABLED="${TLS_DISABLED:-0}"

mkdir -p "$CERT_DIR" /var/cache/nginx /var/run /tmp /etc/nginx/conf.d

if [ "$TLS_DISABLED" = "1" ] || [ "$TLS_DISABLED" = "true" ]; then
  cat > /etc/nginx/conf.d/default.conf <<EOF
server {
    listen ${LISTEN_PORT};
    server_name _;
    root /usr/share/nginx/html;
    index index.html;
    location /api/ {
        proxy_pass ${API_SCHEME}://${API_UPSTREAM};
        proxy_set_header Host \$host;
        proxy_set_header X-Forwarded-Proto \$scheme;
    }
    location /ws/ {
        proxy_pass ${API_SCHEME}://${API_UPSTREAM};
        proxy_http_version 1.1;
        proxy_set_header Upgrade \$http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host \$host;
    }
    location /health { proxy_pass ${API_SCHEME}://${API_UPSTREAM}; }
    location /ready  { proxy_pass ${API_SCHEME}://${API_UPSTREAM}; }
    location / {
        try_files \$uri \$uri/ /index.html;
    }
}
EOF
  exec nginx -g 'daemon off;'
fi

if [ ! -f "$CERT" ] || [ ! -f "$KEY" ]; then
  echo "Generating self-signed TLS certificate for Paqtra UI..."
  openssl req -x509 -nodes -days 3650 \
    -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 \
    -keyout "$KEY" -out "$CERT" \
    -subj "/CN=paqtra/O=Zyvor AI Labs" \
    -addext "subjectAltName=DNS:paqtra,DNS:localhost,IP:127.0.0.1"
  chmod 600 "$KEY" 2>/dev/null || true
fi

export LISTEN_PORT CERT KEY API_UPSTREAM API_SCHEME
envsubst '${LISTEN_PORT} ${CERT} ${KEY} ${API_UPSTREAM} ${API_SCHEME}' \
  < /etc/nginx/templates/default.conf.template \
  > /etc/nginx/conf.d/default.conf

exec nginx -g 'daemon off;'
