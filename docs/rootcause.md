# Root-Cause Engine Module

## Overview

The Root-Cause Engine analyzes packet drops from Cilium eBPF and provides human-readable explanations with actionable fixes. Instead of just seeing "packet dropped", you get the **why** and **how to fix it**.

## Architecture

```
┌──────────────────┐
│  eBPF Drop Map   │  ← Raw drop events from kernel
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│  Event Capture   │  ← Convert to DropEvent
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Pattern Analysis │  ← Find repeating patterns
│  (Analyzer)      │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│  Correlation     │  ← Match with policies/conntrack/lb
│  (Correlator)    │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│  Explanation     │  ← Human-readable text
│  (Explainer)     │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│  Suggested Fix   │  ← Actionable YAML/commands
└──────────────────┘
```

## Components

### 1. Core Engine (`src/modules/rootcause/mod.rs`)

#### DropEvent
Represents a single packet drop with full context:

```rust
pub struct DropEvent {
    pub timestamp: u64,
    pub src_ip: IpAddr,
    pub dst_ip: IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: u8,
    pub reason: DropReason,
    pub identity_src: u32,
    pub identity_dst: u32,
    pub namespace: Option<String>,
    pub pod_name: Option<String>,
}
```

#### DropReason
Enhanced drop reasons with human-readable names:

```rust
pub enum DropReason {
    PolicyDenied,           // CiliumNetworkPolicy denied traffic
    InvalidSourceIP,        // Source IP not recognized
    InvalidPacket,          // Malformed packet
    CTStateMismatch,        // Connection tracking state issue
    PortNotAllowed,         // Port not in policy
    UnknownL3Protocol,      // Unknown Layer 3 protocol
    UnknownL4Protocol,      // Unknown Layer 4 protocol
    NoMapping,              // No endpoint mapping
    UnknownDestination,     // Destination identity not found
    LBError,                // Load balancer error
    ServiceNotFound,        // K8s Service doesn't exist
    NoBackend,              // Service has no healthy pods
    FragNeeded,             // MTU issue (ICMP Frag Needed)
    TTLExceeded,            // Time To Live = 0
    Other(u8),              // Other reason code
}
```

#### RootCauseAnalysis
Complete analysis result:

```rust
pub struct RootCauseAnalysis {
    pub event: DropEvent,
    pub explanation: String,           // Human-readable explanation
    pub likely_cause: String,          // Root cause summary
    pub suggested_fix: SuggestedFix,   // Actionable fix
    pub confidence: f32,               // 0.0 to 1.0
    pub related_policy: Option<String>,// Related CNP name
    pub context: Vec<String>,          // Additional context
}
```

#### SuggestedFix
Actionable fixes with YAML or commands:

```rust
pub enum SuggestedFix {
    AddPolicyRule {
        namespace: String,
        from_labels: HashMap<String, String>,
        to_labels: HashMap<String, String>,
        port: u16,
        protocol: String,
        yaml: String,  // Ready-to-apply CiliumNetworkPolicy
    },

    UpdateMTU {
        interface: String,
        current_mtu: u16,
        suggested_mtu: u16,
        command: String,  // e.g., "ip link set cilium_host mtu 1450"
    },

    FixDNS {
        namespace: String,
        issue: String,
        command: String,
    },

    CheckConntrack {
        issue: String,
        commands: Vec<String>,  // Diagnostic commands
    },

    AddServiceEndpoint {
        service: String,
        namespace: String,
        reason: String,
    },

    FixLoadBalancer {
        service: String,
        namespace: String,
        issue: String,
    },

    ManualInvestigation {
        reason: String,
        steps: Vec<String>,
    },
}
```

#### RootCauseEngine
Main analysis engine:

```rust
pub struct RootCauseEngine<M: MapReader> {
    config: RootCauseConfig,
    ebpf_reader: M,
    k8s_client: K8sClient,
    policy_manager: PolicyManager,

    drop_history: Vec<DropEvent>,
    pattern_counts: HashMap<DropPattern, u64>,
}

impl<M: MapReader> RootCauseEngine<M> {
    pub async fn analyze_drops(&mut self) -> Result<Vec<RootCauseAnalysis>>
    pub fn get_stats(&self) -> DropStats
    pub fn clear_history(&mut self)
}
```

### 2. Explainer (`src/modules/rootcause/explainer.rs`)

Generates human-readable explanations:

```rust
pub struct DropExplainer;

impl DropExplainer {
    pub fn explain(event: &DropEvent) -> String
    pub fn short_summary(event: &DropEvent) -> String
    pub fn technical_details(event: &DropEvent) -> String
}
```

**Example Explanations:**

**Policy Denied:**
```
Policy denied TCP traffic from pod web-app in namespace production
to 10.0.2.15:5432 (identity 100 → 200). No CiliumNetworkPolicy
allows this communication.
```

**No Backend:**
```
Service 10.96.0.10:80 exists but has no healthy backend pods.
Possible reasons:
• All backend pods are not ready (failing health checks)
• Backend pods were recently deleted
• Service selector doesn't match any running pods
• Backend pods exist but on nodes that are unreachable
```

**MTU Issue:**
```
Packet to 10.0.3.20:443 requires fragmentation but DF (Don't Fragment)
flag is set. This is an MTU mismatch issue:
• Source is trying to send packets larger than the path MTU
• Common with overlay networks (VXLAN, Geneve)
• Typical solution: reduce MTU on pod interfaces to 1450 or enable MTU discovery
```

### 3. Correlator (`src/modules/rootcause/correlator.rs`)

Correlates drops with other eBPF data:

```rust
pub struct PolicyCorrelator;

impl PolicyCorrelator {
    // Correlate with policy decisions
    pub async fn correlate_with_policy<M: MapReader>(
        event: &DropEvent,
        ebpf_reader: &M,
    ) -> Result<(Option<String>, Vec<String>)>

    // Correlate with connection tracking
    pub async fn correlate_with_conntrack<M: MapReader>(
        event: &DropEvent,
        ebpf_reader: &M,
    ) -> Result<Vec<String>>

    // Correlate with load balancer state
    pub async fn correlate_with_lb<M: MapReader>(
        event: &DropEvent,
        ebpf_reader: &M,
    ) -> Result<Vec<String>>
}
```

**Correlation Examples:**

```
Found 3 policy rules for source identity 100, but none match destination 200 port 5432

Allowed ports for this source: [80, 443, 8080]

Default-deny policy is active (no egress allowed except explicitly permitted)
```

### 4. Analyzer (`src/modules/rootcause/analyzer.rs`)

Analyzes patterns and trends:

```rust
pub struct DropAnalyzer;

impl DropAnalyzer {
    // Trend analysis
    pub fn analyze_trends(history: &[DropEvent], window_secs: u64) -> TrendAnalysis

    // Find repeating patterns
    pub fn find_repeating_patterns(
        pattern_counts: &HashMap<DropPattern, u64>,
        min_count: u64,
    ) -> Vec<RepeatingPattern>

    // Group by namespace
    pub fn group_by_namespace(events: &[DropEvent]) -> HashMap<String, Vec<DropEvent>>

    // Find top namespaces
    pub fn top_namespaces(events: &[DropEvent], limit: usize) -> Vec<(String, usize)>

    // Reason distribution
    pub fn reason_distribution(events: &[DropEvent]) -> Vec<(DropReason, usize, f32)>

    // Detect anomalies
    pub fn detect_anomalies(
        pattern_counts: &HashMap<DropPattern, u64>,
        baseline_counts: &HashMap<DropPattern, u64>,
    ) -> Vec<Anomaly>

    // Suggest investigations
    pub fn suggest_investigations(
        patterns: &[RepeatingPattern],
    ) -> Vec<Investigation>
}
```

#### Severity Levels

```rust
pub enum Severity {
    Low,       // < 10 drops
    Medium,    // 10-100 drops
    High,      // 100-1000 drops
    Critical,  // > 1000 drops
}
```

#### Anomaly Detection

```rust
pub struct Anomaly {
    pub anomaly_type: AnomalyType,
    pub pattern: DropPattern,
    pub current_count: u64,
    pub baseline_count: u64,
    pub severity: Severity,
}

pub enum AnomalyType {
    NewPattern,           // Pattern never seen before
    IncreasedFrequency,   // 3x increase from baseline
}
```

## Usage Example

```rust
use paqtra::modules::rootcause::*;
use paqtra::ebpf::MockMapReader;
use paqtra::kubernetes::K8sClient;

#[tokio::main]
async fn main() -> Result<()> {
    // Configure root-cause engine
    let config = RootCauseConfig {
        enabled: true,
        analysis_window_secs: 300,  // 5 minutes
        min_drop_count: 5,
        auto_correlate: true,
        pattern_analysis: true,
    };

    // Create engine
    let ebpf_reader = MockMapReader;  // Replace with real CiliumMapReader
    let k8s_client = K8sClient::new().await?;
    let mut engine = RootCauseEngine::new(config, ebpf_reader, k8s_client);

    // Analyze drops
    let analyses = engine.analyze_drops().await?;

    for analysis in &analyses {
        println!("\n🔍 Drop Analysis:");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        // Explanation
        println!("\n{}", analysis.explanation);

        // Likely cause
        println!("\n📍 Likely Cause:");
        println!("   {}", analysis.likely_cause);

        // Confidence
        println!("\n🎯 Confidence: {:.0}%", analysis.confidence * 100.0);

        // Suggested fix
        println!("\n✨ Suggested Fix:");
        match &analysis.suggested_fix {
            SuggestedFix::AddPolicyRule { yaml, .. } => {
                println!("\nApply this policy:\n");
                println!("{}", yaml);
            }
            SuggestedFix::UpdateMTU { command, .. } => {
                println!("\nRun: {}", command);
            }
            SuggestedFix::CheckConntrack { commands, .. } => {
                println!("\nRun these commands:");
                for cmd in commands {
                    println!("  {}", cmd);
                }
            }
            _ => println!("   See details above"),
        }

        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    }

    // Get statistics
    let stats = engine.get_stats();
    println!("\n📊 Drop Statistics:");
    println!("   Total drops: {}", stats.total_drops);
    println!("   By reason:");
    for (reason, count) in &stats.by_reason {
        println!("     {}: {}", reason.to_string(), count);
    }

    Ok(())
}
```

## Example Output

### Policy Denied Drop

```
🔍 Drop Analysis:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Policy denied TCP traffic from pod web-app in namespace production
to 10.0.2.15:5432 (identity 100 → 200). No CiliumNetworkPolicy
allows this communication.

📍 Likely Cause:
   No CiliumNetworkPolicy allows this traffic

🎯 Confidence: 95%

✨ Suggested Fix:

Apply this policy:

apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: allow-traffic-fix
  namespace: production
spec:
  endpointSelector:
    matchLabels:
      security.identity: "100"
  egress:
  - toEndpoints:
    - matchLabels:
        security.identity: "200"
    toPorts:
    - ports:
      - port: "5432"
        protocol: TCP

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

### Service Not Found Drop

```
🔍 Drop Analysis:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Service for 10.96.0.10:80 was not found in the load balancer map.
This means:
• No Kubernetes Service exists for this destination
• Service exists but Cilium hasn't synced it yet
• Service selector doesn't match any pods

📍 Likely Cause:
   Service does not exist or is not registered

🎯 Confidence: 85%

✨ Suggested Fix:
   Check if service exists: kubectl get svc --all-namespaces
   Verify service endpoints: kubectl get endpoints
   Check Cilium service sync: cilium service list

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

## Pattern Analysis

```rust
use paqtra::modules::rootcause::analyzer::*;

// Find repeating patterns
let patterns = DropAnalyzer::find_repeating_patterns(&pattern_counts, 10);

for pattern in patterns {
    println!("Pattern: {}", pattern.description);
    println!("Occurrences: {}", pattern.occurrences);
    println!("Severity: {:?}", pattern.severity);
}

// Output:
// Pattern: Policy Denied - identity 100 → 200 on port 5432 (TCP)
// Occurrences: 142
// Severity: High
```

## Trend Analysis

```rust
let trends = DropAnalyzer::analyze_trends(&drop_history, 300);

println!("Total drops: {}", trends.total_drops);
println!("Drop rate: {:.2} drops/sec", trends.drop_rate_per_sec);
println!("Spike detected: {}", trends.is_spike);
println!("Most common: {:?}", trends.most_common_reason);

// Output:
// Total drops: 523
// Drop rate: 1.74 drops/sec
// Spike detected: false
// Most common: Some(PolicyDenied)
```

## Anomaly Detection

```rust
let anomalies = DropAnalyzer::detect_anomalies(&current_patterns, &baseline_patterns);

for anomaly in anomalies {
    match anomaly.anomaly_type {
        AnomalyType::NewPattern => {
            println!("⚠️  NEW drop pattern detected!");
        }
        AnomalyType::IncreasedFrequency => {
            println!("📈 Drop frequency increased 3x!");
            println!("   Baseline: {}", anomaly.baseline_count);
            println!("   Current: {}", anomaly.current_count);
        }
    }
}
```

## Investigation Suggestions

```rust
let investigations = DropAnalyzer::suggest_investigations(&patterns);

for inv in investigations {
    println!("\n🔬 {}", inv.title);
    println!("   Priority: {:?}", inv.priority);
    println!("   Steps:");
    for step in &inv.steps {
        println!("     • {}", step);
    }
}

// Output:
// 🔬 Investigate policy gaps for port 5432
//    Priority: High
//    Steps:
//      • Check if traffic from identity 100 to 200 should be allowed
//      • Review CiliumNetworkPolicy rules
//      • Consider adding explicit allow rule or verify deny is intentional
```

## Integration with Other Modules

### With Self-Healer

```rust
// Root-cause identifies the problem
let analyses = rootcause_engine.analyze_drops().await?;

// Self-healer can auto-apply the fix
for analysis in analyses {
    if let SuggestedFix::AddPolicyRule { yaml, .. } = &analysis.suggested_fix {
        if analysis.confidence > 0.9 {
            healer.apply_fix(yaml).await?;
        }
    }
}
```

### With AutoPolicy

```rust
// Root-cause finds policy gaps
// AutoPolicy learns correct traffic patterns
// Together they suggest and validate fixes
```

## Configuration

```rust
pub struct RootCauseConfig {
    pub enabled: bool,                    // Enable/disable engine
    pub analysis_window_secs: u64,        // How far back to analyze (default: 300)
    pub min_drop_count: u64,              // Min drops to report (default: 5)
    pub auto_correlate: bool,             // Auto-correlate with policies (default: true)
    pub pattern_analysis: bool,           // Enable pattern detection (default: true)
}
```

## Performance

- **Memory:** O(drops in window) - typically 100s to 1000s
- **CPU:** Minimal - analysis on-demand
- **Latency:** < 100ms for typical analysis

For high-traffic clusters:
- Increase `min_drop_count` to reduce noise
- Decrease `analysis_window_secs` to limit history
- Disable `pattern_analysis` if not needed

## Testing

Run root-cause tests:
```bash
cargo test rootcause
```

Tests include:
- ✓ Drop reason conversion
- ✓ Root-cause engine creation
- ✓ Explanation generation
- ✓ Policy correlation
- ✓ Trend analysis
- ✓ Severity calculation
- ✓ Pattern description
- ✓ Reason distribution

## Troubleshooting

### No drops detected
- Check eBPF map access: `cilium bpf drop list`
- Verify Cilium monitor is running
- Ensure traffic is actually being dropped

### Low confidence scores
- Increase `analysis_window_secs` for more history
- Wait for more occurrences
- Check if pattern is truly repeating

### Wrong suggested fixes
- May need more context (pod labels, namespace info)
- IPCache resolution not yet implemented
- File an issue with the actual vs expected fix

## Future Enhancements

### v1.1 - Enhanced Resolution
- [ ] IPCache integration for pod label resolution
- [ ] Namespace and pod name auto-detection
- [ ] Better service-to-pod mapping

### v1.2 - ML-based Analysis
- [ ] Learn normal drop patterns
- [ ] Predict future issues
- [ ] Smarter confidence scoring

### v1.3 - Auto-remediation
- [ ] Safe auto-apply of high-confidence fixes
- [ ] Rollback mechanism
- [ ] Fix validation

### v1.4 - Advanced Correlation
- [ ] Correlate with application logs
- [ ] Link to specific deployments
- [ ] Timeline visualization

## Related Modules

- **Self-Healer:** Auto-applies fixes from root-cause analysis
- **AutoPolicy:** Learns correct patterns to prevent future drops
- **Traffic Replay:** Replay dropped traffic after fix
- **What-If Simulator:** Test fixes before applying

## Status

✅ **Completed (v1.0)**
- Core drop analysis
- Human-readable explanations
- Suggested fixes (YAML + commands)
- Policy correlation
- Pattern analysis
- Trend detection
- Anomaly detection
- Tests passing

⏳ **In Progress**
- IPCache resolution
- TUI integration

📋 **Planned**
- ML-based improvements
- Auto-remediation
- Timeline visualization
