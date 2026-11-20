# Scenario: Safe Canary Deployment Without Sidecars

## Objective
Deploy new version of `frontend-app` (v2.1) using progressive traffic shifting with automated health checks.

## Prerequisites
- Cilium 1.14+ installed
- L7 policy enforcement enabled
- Two deployments ready: `frontend-v2.0` (stable) and `frontend-v2.1` (canary)

## Step-by-Step Canary Deployment

### 1. Navigate to Canary Tab
```
Press Tab until you reach "Canary" tab
```

### 2. Create Canary (External - via kubectl)

First, ensure both versions are deployed:
```bash
# Stable version (currently serving 100%)
kubectl get deployment frontend-v2.0

# New canary version (ready but no traffic)
kubectl get deployment frontend-v2.1
```

### 3. Monitor Canary in TUI

Initial state:
```
📊 Traffic Distribution (frontend-v2.1)

Stable (v2.0):  ██████████████████████ 90%
Canary (v2.1):  ██░░░░░░░░░░░░░░░░░░░░ 10%

Metrics:
Canary Success:  99.8%  ✅
Canary Latency:  42ms   (vs 45ms stable)
Error Rate:      0.2%   (threshold: 1%)

Health: ████████████████████ 99.8%
Status: Running ✓
Recommendation: Continue (looks healthy)
```

### 4. Progressive Traffic Shifting

**Increase traffic by 10%**:
```
Press '+' to progress traffic
```

New state (20%):
```
Stable (v2.0):  ████████████████░░░░░░ 80%
Canary (v2.1):  ████░░░░░░░░░░░░░░░░░░ 20%

Metrics:
Canary Success:  99.5%  ✅
Canary Latency:  43ms   (vs 45ms stable)
Error Rate:      0.5%   (threshold: 1%)

Health: ███████████████████░ 99.5%
Recommendation: Continue
```

**Continue progression**:
```
Press '+' repeatedly to increase: 20% → 30% → 40% → 50%
```

At 50% traffic:
```
Stable (v2.0):  ██████████░░░░░░░░░░░░ 50%
Canary (v2.1):  ██████████░░░░░░░░░░░░ 50%

Metrics:
Canary Success:  99.3%  ✅
Canary Latency:  44ms   (vs 45ms stable)
Error Rate:      0.7%   (threshold: 1%)

Health: ██████████████████░░ 99.3%
Recommendation: Continue (canary performing well)
```

### 5. Full Promotion

When confident (90%+ traffic):
```
Press 'p' to promote canary
Press 'y' to confirm promotion
```

Result:
```
Stable (v2.0):  ░░░░░░░░░░░░░░░░░░░░░░ 0%
Canary (v2.1):  ██████████████████████ 100%

Status: Promoted ✅
v2.1 is now the stable version!
```

---

## Scenario: Canary with Issues - Auto Rollback

### Problem Detected

At 40% traffic, canary shows issues:
```
Stable (v2.0):  ████████████░░░░░░░░░░ 60%
Canary (v2.1):  ████████░░░░░░░░░░░░░░ 40%

Metrics:
Canary Success:  85.2%  ❌ (below threshold!)
Canary Latency:  280ms  (vs 45ms stable) ⚠️
Error Rate:      14.8%  (threshold: 1%)

Health: ████████░░░░░░░░░░░░ 85.2%
Recommendation: Rollback ⚠️
```

### Manual Rollback
```
Press 'r' to initiate rollback
Press 'y' to confirm
```

Result:
```
Rolling back to v2.0...

Stable (v2.0):  ██████████████████████ 100%
Canary (v2.1):  ░░░░░░░░░░░░░░░░░░░░░░ 0%

Status: Rolled Back ✅
Reason: Error rate exceeded threshold (14.8% > 1%)

Action: Fix v2.1 issues before re-deploying
```

---

## Advanced: Canary with A/B Testing

### Target Specific Users

Canary to premium users only (external config):
```yaml
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: frontend-canary-premium
spec:
  endpointSelector:
    matchLabels:
      app: frontend
  ingress:
  - toPorts:
    - ports:
      - port: "80"
        protocol: TCP
      rules:
        http:
        - headers:
          - "X-User-Tier: premium"
          method: "GET"
          path: "/.*"
    # Route to canary backend
    toEndpoints:
    - matchLabels:
        app: frontend
        version: v2.1
```

Monitor in TUI:
```
Traffic Distribution (Premium Users Only):
Canary receives only requests with X-User-Tier: premium header
```

---

## Comparison: Sidecar vs Sidecarless

### Traditional Sidecar Approach (Istio/Linkerd)

**Overhead**:
- CPU: 50-100m per pod
- Memory: 50-100 MB per pod
- Latency: +2-5ms per hop

**For 100 pods**:
- CPU: 5-10 cores wasted
- Memory: 5-10 GB wasted
- Cost: $500-1000/month

### Cilium Sidecarless Approach

**Overhead**:
- CPU: <1% per node (eBPF)
- Memory: Negligible
- Latency: <0.1ms (kernel space)

**For 100 pods**:
- CPU: 0.1-0.2 cores total
- Memory: <100 MB total
- Cost: ~$10/month

**Savings**: 98% reduction in cost and overhead! 🎉

---

## Best Practices

1. **Start Small**: Begin with 10% traffic
2. **Incremental**: Progress in 10% steps
3. **Monitor**: Watch success rate and latency continuously
4. **Conservative Thresholds**: Set auto-rollback at 90% success
5. **Health Checks**: Ensure robust health endpoints
6. **Gradual**: Don't rush - wait 5-10 minutes between increments
7. **Rollback Plan**: Always be ready to rollback

---

## Configuration

Recommended canary settings:
```yaml
canary:
  enabled: true
  initial_traffic_pct: 10          # Start small
  traffic_step_pct: 10              # Incremental
  auto_promote_threshold: 0.99      # 99% success = promote
  auto_rollback_threshold: 0.90     # 90% success = rollback
  health_check_interval_secs: 30    # Frequent checks
  metrics_interval_secs: 10         # Real-time metrics
```

For production:
```yaml
canary:
  initial_traffic_pct: 5            # Even smaller start
  traffic_step_pct: 5
  auto_promote_threshold: 0.995     # 99.5% required
  auto_rollback_threshold: 0.95     # Aggressive rollback
```
