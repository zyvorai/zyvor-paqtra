# ─── Stage 1: Build React App ────────────────────────────────
FROM docker.io/library/node:22-alpine AS builder
WORKDIR /ui
COPY package*.json ./
RUN npm ci --silent
COPY . ./
RUN npm run build

# ─── Stage 2: Serve with Nginx (HTTPS by default, like Netra) ─
FROM docker.io/library/nginx:alpine

RUN apk add --no-cache openssl gettext \
 && addgroup -g 1000 paqtra && adduser -D -u 1000 -G paqtra paqtra \
 && mkdir -p /var/cache/nginx /var/run /var/log/nginx /certs /etc/nginx/templates \
 && chown -R paqtra:paqtra /var/cache/nginx /var/run /var/log/nginx /usr/share/nginx/html /certs /etc/nginx/conf.d \
 && rm -f /etc/nginx/conf.d/default.conf

COPY --from=builder --chown=paqtra:paqtra /ui/dist /usr/share/nginx/html
COPY nginx.conf.template /etc/nginx/templates/default.conf.template
COPY docker-entrypoint.sh /docker-entrypoint.sh
RUN chmod +x /docker-entrypoint.sh

ENV UI_PORT=8443 \
    API_UPSTREAM=paqtra-api:9191 \
    API_SCHEME=http \
    TLS_CERT_DIR=/certs

USER paqtra
EXPOSE 8443

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD wget --quiet --tries=1 --no-check-certificate --spider https://127.0.0.1:8443/ || exit 1

ENTRYPOINT ["/docker-entrypoint.sh"]
