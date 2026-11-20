# Scenario: Debugging Production Incident with Time-Travel

## Problem
Production service `payment-api` started failing at 14:32 UTC. Users report intermittent 500 errors.

## Investigation Steps Using Paqtra

### 1. Navigate to Replay Tab
```
Press Tab until you reach the "Replay" tab
```

### 2. Enter Time-Travel Mode
```
Press 't' to enter time-travel debugging
```

### 3. Navigate to Incident Time
```
Use arrow keys (←/→) to scrub timeline to 14:32 UTC
Look for event markers (!) indicating drops or errors
```

### 4. Analyze Network State
The time-travel view shows:
```
🌐 Network State at Flow #8,450 (14:32:15 UTC)

Active Connections:    47
Allowed Flows:         42
Dropped Flows:         5  ← SPIKE!
Unique Endpoints:      18
Active Policies:       12

Latest Event:          DROP at payment-api → postgres-db:5432
Reason:                Policy denied (no matching rule)
```

### 5. Identify Root Cause
```
Press '[' to jump to the previous drop event
Press ']' to jump to the next drop event
```

**Finding**: Database connection policy was modified at 14:31 UTC, removing payment-api's access.

### 6. Explain the Packet
```
Press 'e' on the dropped flow to get detailed analysis
```

**Explanation**:
```
🔍 What Happened:
Traffic was BLOCKED from payment-api to postgres-db on port 5432

💡 Why:
PostgreSQL traffic blocked. CiliumNetworkPolicy "db-access"
was recently modified and no longer includes payment-api selector.

🔧 Troubleshooting:
1. Check recent policy changes in last 10 minutes
2. Verify payment-api labels match policy selector
3. Add ingress rule allowing payment-api → postgres-db:5432
4. Check RootCause tab for recommended fix
```

### 7. Apply Fix via RootCause Tab
```
Press Tab to navigate to "RootCause" tab
Press '↓' to select the recommended fix
Press 'a' to apply the fix
Press 'y' to confirm
```

**Fix Applied**:
```yaml
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: db-access
spec:
  endpointSelector:
    matchLabels:
      app: postgresql
  ingress:
  - fromEndpoints:
    - matchLabels:
        app: payment-api  # Re-added
```

### 8. Verify Resolution
```
Navigate back to Flows tab
Observe traffic now FORWARDED from payment-api → postgres-db
```

## Time to Resolution
- **Without Paqtra**: 45-60 minutes (log aggregation, kubectl debugging, trial and error)
- **With Time-Travel**: 5-8 minutes (visual timeline, instant root cause, one-click fix)

## Prevention
Enable AutoPolicy tab to catch this automatically:
```
Press Tab to navigate to "AutoPolicy"
Press 'u' to update learning from current flows
Press 'g' to generate policies with ML confidence
```

AutoPolicy will detect this pattern and recommend the policy with 95%+ confidence.
