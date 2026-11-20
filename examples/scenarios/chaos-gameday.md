# Scenario: Chaos Engineering GameDay

## Objective
Test microservices resilience during a controlled GameDay exercise.

## Setup
- **Duration**: 2 hours
- **Scope**: Staging environment
- **Team**: SRE + Development
- **Goals**:
  - Validate retry logic
  - Test circuit breakers
  - Verify graceful degradation
  - Measure recovery time

## Experiments Using Paqtra

### Experiment 1: Network Partition (15 minutes)

**Navigate to Chaos Tab**:
```
Press Tab until you reach "Chaos" tab
Press '↓' to select "Network Partition"
```

**Preset Details**:
```
🌪️  Network Partition

Type:        Packet Drop
Rate:        20%
Target:      All namespaces, egress traffic
Duration:    5 minutes (auto-cleanup)
Severity:    Medium

What It Tests:
Simulates network instability and packet loss.
Tests application retry logic and resilience.
```

**Execute**:
```
Press Enter to run experiment
Confirm with 'y'
```

**Observe**:
- Monitor "Connections" tab for connection failures
- Check application dashboards for retry attempts
- Verify services remain available (degraded performance OK)

**Expected Results**:
- ✅ Applications retry failed requests
- ✅ No cascading failures
- ✅ Recovery within 2-3 seconds
- ❌ Some services timeout (fix needed!)

**Cleanup**:
Auto-cleanup after 5 minutes, or press 's' to stop manually.

---

### Experiment 2: Latency Spike (15 minutes)

**Select Preset**:
```
Press '↓' to select "Latency Spike"
Press Enter
```

**Details**:
```
Type:        Latency Injection
Delay:       500ms ± 100ms jitter
Target:      All egress traffic
Duration:    5 minutes
Severity:    Medium
```

**Observe**:
- Monitor P50, P95, P99 latencies
- Check timeout configurations
- Verify circuit breaker activation

**Expected Results**:
- ✅ Circuit breakers open after threshold
- ✅ Fallback mechanisms activate
- ✅ User experience degrades gracefully
- ❌ Some requests timeout (increase timeout configs!)

---

### Experiment 3: DNS Outage (20 minutes)

**Select Preset**:
```
Press '↓' to select "DNS Outage"
Press Enter
```

**Details**:
```
Type:        DNS Failures
Failure Rate: 30%
Target:      DNS queries
Duration:    5 minutes
Severity:    High ⚠️
```

**Observe**:
- Check DNS caching effectiveness
- Monitor service discovery failures
- Verify connection pooling reduces DNS lookups

**Expected Results**:
- ✅ DNS caching reduces impact
- ✅ Existing connections unaffected
- ✅ Services use connection pools
- ❌ New service discovery fails (expected)

---

### Experiment 4: Connection Reset (15 minutes)

**Select Preset**:
```
Press '↓' to select "Connection Reset"
Press Enter
```

**Details**:
```
Type:        Connection Termination
Kill Rate:   15%
Target:      Active connections
Duration:    5 minutes
Severity:    Medium
```

**Observe**:
- Monitor reconnection attempts
- Check connection pool health
- Verify graceful error handling

**Expected Results**:
- ✅ Applications reconnect automatically
- ✅ Exponential backoff observed
- ✅ No data corruption
- ❌ Some requests fail (retry logic working!)

---

### Emergency: Circuit Breaker

If any experiment causes unexpected issues:

```
Press 'b' to trigger circuit breaker
This immediately stops ALL chaos experiments
```

Or stop individual experiment:
```
Press 'v' to switch to active experiments view
Press '↓' to select experiment
Press 's' to stop it
```

---

## Post-GameDay Analysis

### Navigate to Time-Travel
```
Press Tab to "Replay" tab
Press 't' to enter time-travel mode
```

### Review Timeline
```
Use ←/→ to scrub through the GameDay timeline
Look for event markers (!) where failures occurred
Press '[' and ']' to jump between significant events
```

### Generate Report

**Findings**:
1. ✅ Retry logic works for 80% of services
2. ❌ Payment service needs timeout increase (500ms → 2000ms)
3. ✅ Circuit breakers activate correctly
4. ❌ Cache service doesn't handle DNS failures gracefully
5. ✅ Database connection pooling effective

**Action Items**:
- Increase payment-service timeout configuration
- Implement DNS caching in cache service
- Add health checks to detect degraded state faster
- Document circuit breaker thresholds

---

## Best Practices

1. **Start Small**: Begin with low severity experiments
2. **Incremental**: Progress from 10% → 20% → 50% impact
3. **Monitor**: Watch metrics continuously during experiments
4. **Document**: Record observations and failures
5. **Circuit Breaker**: Always have emergency stop ready
6. **Auto-Cleanup**: Let experiments auto-cleanup (don't forget!)
7. **Team Communication**: Keep team informed during experiments

---

## Configuration for GameDays

Use development-style chaos config:
```yaml
chaos:
  enabled: true
  max_drop_rate: 0.5
  max_latency_ms: 5000
  max_duration_secs: 600
  auto_cleanup: true
  auto_cleanup_duration: 300
  require_confirmation: true  # Safety!
  circuit_breaker_enabled: true
```

**NEVER** run chaos experiments in production without:
- Change management approval
- Customer communication
- Incident response team on standby
- Rollback plan ready
