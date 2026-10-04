# Paqtra Web Application - Deployment Guide

## Quick Start

### Prerequisites
- Kubernetes cluster with Cilium installed
- kubectl configured
- Docker (for building images)
- Helm 3+ (optional, for production deployment)

## Deployment Options

### Option 1: Docker Compose (Development)

**For local development and testing:**

```bash
# Navigate to deployment directory
cd deployments

# Start all services
docker-compose up -d

# Check status
docker-compose ps

# View logs
docker-compose logs -f

# Stop services
docker-compose down
```

Access the application:
- **Frontend**: http://localhost:3000
- **Backend API**: http://localhost:9191
- **Redis**: localhost:6379

### Option 2: Kubernetes (Production)

#### Step 1: Build Docker Images

```bash
# Build backend image
cd web-api
docker build -t paqtra-api:latest .

# Build frontend image
cd ../web-ui
docker build -t paqtra-ui:latest .

# Tag and push to registry (replace with your registry)
docker tag paqtra-api:latest your-registry.com/paqtra-api:1.0.0
docker tag paqtra-ui:latest your-registry.com/paqtra-ui:1.0.0

docker push your-registry.com/paqtra-api:1.0.0
docker push your-registry.com/paqtra-ui:1.0.0
```

#### Step 2: Update Kubernetes Manifests

Edit `deployments/k8s/backend-deployment.yaml` and `deployments/k8s/frontend-deployment.yaml`:

```yaml
# Change image references
image: your-registry.com/paqtra-api:1.0.0  # backend
image: your-registry.com/paqtra-ui:1.0.0   # frontend
```

Edit `deployments/k8s/ingress.yaml`:

```yaml
# Update hostname
host: paqtra.your-domain.com
```

#### Step 3: Deploy to Kubernetes

```bash
# Create namespace (if not exists)
kubectl create namespace cilium-system

# Deploy secrets (IMPORTANT: Change JWT secret!)
kubectl apply -f deployments/k8s/secrets.yaml

# Deploy configuration
kubectl apply -f deployments/k8s/configmap.yaml

# Deploy RBAC
kubectl apply -f deployments/k8s/rbac.yaml

# Deploy backend API
kubectl apply -f deployments/k8s/backend-deployment.yaml

# Deploy frontend UI
kubectl apply -f deployments/k8s/frontend-deployment.yaml

# Deploy ingress
kubectl apply -f deployments/k8s/ingress.yaml
```

#### Step 4: Verify Deployment

```bash
# Check pod status
kubectl get pods -n cilium-system -l app=paqtra

# Check services
kubectl get svc -n cilium-system -l app=paqtra

# View logs
kubectl logs -n cilium-system -l app=paqtra,component=api
kubectl logs -n cilium-system -l app=paqtra,component=ui

# Test backend health
kubectl port-forward -n cilium-system svc/paqtra-api 9191:9191
curl http://localhost:9191/health

# Test frontend
kubectl port-forward -n cilium-system svc/paqtra-ui 3000:80
# Open browser to http://localhost:3000
```

#### Step 5: Access the Application

After deploying ingress:

```bash
# Get ingress address
kubectl get ingress -n cilium-system paqtra

# Access via ingress hostname
# https://paqtra.your-domain.com
```

### Option 3: `paqtra install` or the Helm chart (Recommended for Production)

```bash
# The CLI has the chart built in (see docs/cli.md)
paqtra install --namespace paqtra \
  --set api.env.hubbleAddress=hubble-relay.kube-system.svc.cluster.local:80 \
  --set ingress.enabled=true

# Mirrored registry
paqtra install --registry your-registry.com/zyvorai

# Upgrade / uninstall
paqtra upgrade
paqtra uninstall

# Or plain Helm, from the published chart or a checkout
helm install paqtra oci://ghcr.io/zyvorai/charts/paqtra --version <X.Y.Z> -n paqtra --create-namespace
helm install paqtra ./chart -n paqtra --create-namespace
```

## Configuration

### Environment Variables

**Backend (API)**:
- `PAQTRA_HOST`: Bind address (default: 0.0.0.0)
- `PAQTRA_PORT`: Server port (default: 9191)
- `JWT_SECRET`: JWT signing secret (change in production!)
- `HUBBLE_ADDRESS`: Hubble Relay address (`host:port`, plaintext gRPC; in-cluster this is the `hubble-relay` service)
- `HUBBLE_MODE`: Helm chart defaults to `grpc` (Observer API only). `auto` reads flows over gRPC and uses the `hubble` CLI only if gRPC fails and the binary is installed; `cli` only uses the binary. No `hubble` binary is needed for `grpc`.
- `PROMETHEUS_URL`: Prometheus base URL (optional; chart value `api.env.prometheusUrl`). Enables `/api/v1/hubble/metrics` and `/api/v1/cilium/metrics`; without it they answer `available: false`. Hubble series exist only for the metrics listed in Cilium's `hubble.metrics.enabled`.
- `K8S_CONTEXT`: Kubernetes context (optional)
- `RUST_LOG`: Log level (info, debug, trace)

**Frontend (UI)**:
- `API_URL`: Backend API URL

### Secrets Management

**IMPORTANT**: Change default secrets in production!

```bash
# Generate secure JWT secret
JWT_SECRET=$(openssl rand -base64 64)

# Create Kubernetes secret
kubectl create secret generic paqtra-secrets \
  --namespace cilium-system \
  --from-literal=jwt-secret="${JWT_SECRET}"
```

### TLS/HTTPS Setup

Using cert-manager for automatic certificate management:

```bash
# Install cert-manager
kubectl apply -f https://github.com/cert-manager/cert-manager/releases/download/v1.13.0/cert-manager.yaml

# Create ClusterIssuer for Let's Encrypt
cat <<EOF | kubectl apply -f -
apiVersion: cert-manager.io/v1
kind: ClusterIssuer
metadata:
  name: letsencrypt-prod
spec:
  acme:
    server: https://acme-v02.api.letsencrypt.org/directory
    email: your-email@example.com
    privateKeySecretRef:
      name: letsencrypt-prod
    solvers:
    - http01:
        ingress:
          class: nginx
EOF

# Ingress will automatically request certificate
```

## Scaling

### Horizontal Pod Autoscaling

```bash
# Backend API autoscaling
kubectl autoscale deployment paqtra-api \
  --namespace cilium-system \
  --cpu-percent=70 \
  --min=3 \
  --max=10

# Frontend UI autoscaling
kubectl autoscale deployment paqtra-ui \
  --namespace cilium-system \
  --cpu-percent=70 \
  --min=2 \
  --max=5
```

## Monitoring

### Prometheus Metrics

The API exposes Prometheus metrics at `/metrics`:

```yaml
# ServiceMonitor for Prometheus Operator
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: paqtra-api
  namespace: cilium-system
spec:
  selector:
    matchLabels:
      app: paqtra
      component: api
  endpoints:
  - port: http
    path: /metrics
```

### Grafana Dashboards

Import pre-built dashboards from `deployments/grafana/`.

### Logging

Logs are output in JSON format for easy ingestion:

```bash
# View structured logs
kubectl logs -n cilium-system -l app=paqtra,component=api | jq .

# Forward to Elasticsearch
# Use Fluent Bit or Fluentd with proper filters
```

## Troubleshooting

### Common Issues

**1. Backend can't connect to Hubble**
```bash
# Check Hubble Relay is running
kubectl get pods -n kube-system -l k8s-app=hubble-relay

# Test connectivity
kubectl run test --rm -it --image=curlimages/curl -- \
  curl -v telnet://hubble-relay.kube-system.svc.cluster.local:4245
```

**2. Frontend can't reach backend**
```bash
# Check backend service
kubectl get svc -n cilium-system paqtra-api

# Test from within cluster
kubectl run test --rm -it --image=curlimages/curl -- \
  curl http://paqtra-api.cilium-system.svc.cluster.local:9191/health
```

**3. Permission denied errors**
```bash
# Check RBAC
kubectl get clusterrolebinding paqtra-api

# Verify ServiceAccount
kubectl get sa -n cilium-system paqtra-api
```

### Debug Mode

Enable debug logging:

```bash
# Backend
kubectl set env deployment/paqtra-api \
  RUST_LOG=trace,paqtra_api=trace \
  -n cilium-system

# View detailed logs
kubectl logs -f -n cilium-system -l component=api
```

## Security Best Practices

1. **Change default secrets** - Never use default JWT_SECRET in production
2. **Use RBAC** - Limit ServiceAccount permissions to minimum required
3. **Enable TLS** - Always use HTTPS in production
4. **Network Policies** - Restrict pod-to-pod communication
5. **Image Scanning** - Scan container images for vulnerabilities
6. **Resource Limits** - Set appropriate CPU/memory limits
7. **Pod Security Standards** - Use restrictive security contexts

## Performance Tuning

### Backend API

With `api.persistence.enabled` (durable flow index), give the API enough RAM for
SQLite + concurrent bpftool inventory. Chart defaults:

```yaml
api:
  resources:
    requests:
      memory: "512Mi"
      cpu: "100m"
    limits:
      memory: "2Gi"
      cpu: "1000m"
```

Example production-oriented sizing:

```yaml
resources:
  requests:
    memory: "512Mi"
    cpu: "500m"
  limits:
    memory: "2Gi"
    cpu: "2000m"
```

### Node agent

The agent DaemonSet collects per-second metrics (see
[metrics.md](metrics.md#sizing)). Chart defaults, sized for ~20k series per
node (100+ pods with per-container cgroups):

```yaml
agent:
  resources:
    requests:
      memory: "128Mi"
      cpu: "100m"
    limits:
      memory: "512Mi"
      cpu: "1"
```

The chart also sets `MALLOC_ARENA_MAX=2` on the agent to keep glibc's
per-thread arenas from doubling its memory.

### Redis
```yaml
resources:
  requests:
    memory: "256Mi"
    cpu: "250m"
  limits:
    memory: "1Gi"
    cpu: "1000m"
```

### Frontend
```yaml
resources:
  requests:
    memory: "128Mi"
    cpu: "200m"
  limits:
    memory: "512Mi"
    cpu: "1000m"
```

## Backup & Restore

### Configuration Backup
```bash
# Backup all configurations
kubectl get all,secret,configmap,ingress -n cilium-system \
  -l app=paqtra -o yaml > backup.yaml

# Restore
kubectl apply -f backup.yaml
```

### Redis Data Backup
```bash
# For persistent Redis, backup PVC
kubectl get pvc -n cilium-system

# Use Velero for full backup solution
```

## Production Checklist

- [ ] Change JWT_SECRET from default
- [ ] Configure TLS/HTTPS
- [ ] Set up monitoring (Prometheus + Grafana)
- [ ] Configure logging aggregation
- [ ] Enable autoscaling
- [ ] Set resource requests/limits
- [ ] Deploy Redis HA
- [ ] Configure network policies
- [ ] Set up backups
- [ ] Test disaster recovery
- [ ] Document runbooks
- [ ] Set up alerting

---

For more information, see:
- [Architecture](web-architecture.md)
- [Web app](web-app.md)
- [API reference](client/api-reference.html)
- [Cilium brotherhood](cilium-brotherhood.md)
