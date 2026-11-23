# Setura Gateway in front of Paqtra (Gateway API)

Paqtra can sit behind [Setura Gateway](https://github.com/zyvorai/zyvor-setura-gateway)
through the Kubernetes Gateway API. Any Gateway API implementation works the
same way. It is off by default and does not change Paqtra's own
authentication (JWT, API key). The gateway adds TLS termination, hostname
routing and edge policy.

Node agents keep talking to the in-cluster Services directly.

## Install the gateway

The cluster needs the Gateway API CRDs and the Setura controller (GatewayClass
`setura`, controller `zyvor.dev/setura-gateway`):

```bash
setura gateway install --image ghcr.io/zyvorai/zyvor-setura-gateway:0.7.0 \
  --namespace setura-system | kubectl apply -f -
```

Then create a `Gateway` of class `setura` with an HTTPS listener (your TLS
Secret) for the hostname you want to serve. Add `--expose-gateway-ports` to the
install if the listener uses a port other than 80 or 443.

## Enable

```bash
helm upgrade --install paqtra ./chart -n paqtra \
  --set gateway.enabled=true \
  --set 'gateway.parentRefs[0].name=setura' \
  --set 'gateway.parentRefs[0].namespace=default' \
  --set 'gateway.hostnames[0]=paqtra.example.com' \
  --set tls.enabled=false --set gateway.backendTLS.enabled=false
```

`gateway.parentRefs` is required when the gateway is enabled; the chart fails
fast without it.

Routes: `/api` and `/swagger-ui` go to the API Service, `/` goes to the UI
Service (`gateway.apiPaths`, `gateway.uiPath`).

## Backend TLS

The UI serves HTTPS by default with a per-pod self-signed certificate. The
gateway cannot verify that, so behind it either:

- set `tls.enabled=false` and `gateway.backendTLS.enabled=false` and let the
  gateway terminate TLS (the pod hop is in-cluster HTTP), or
- keep `tls.enabled=true` and supply the CA of a certificate you control in
  `gateway.backendTLS.caCertificateRefs`; the chart then renders a
  `BackendTLSPolicy` for the UI Service.

The chart refuses to render the half-configured combinations.

## WebSockets over backend TLS

Setura handles WebSocket and other Upgrade requests itself, so no extra policy
object is needed. The chart no longer renders the `BackendTrafficPolicy` of
Envoy Gateway (`gateway.backendTrafficPolicy` is removed). This has been run
on the Netra chart; it has not been exercised on Paqtra yet.

## Streams

Flow and metric streams are long-lived. `gateway.requestTimeout` defaults to
`"0s"` so the gateway does not cut them.

## Not covered

Setura is a front door here, not a data-plane integration. Rate limits and IP
allow lists are configured in Setura itself (`--rate-limit`, auth blocks), not
through this chart.
