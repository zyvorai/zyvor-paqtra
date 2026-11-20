# AutoPolicy Module - Zero-Trust Policy Learning

## Overview

The AutoPolicy module automatically learns traffic patterns from eBPF and generates minimal privilege CiliumNetworkPolicy resources. This enables zero-trust security without manual policy writing.

## Architecture

```
┌─────────────────┐
│  eBPF Maps      │
│  (Conntrack)    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ TrafficLearner  │  ← Observes patterns over time
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ TrafficAnalyzer │  ← Builds graphs, detects issues
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ PolicyGenerator │  ← Generates CNP YAML
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ Apply to K8s    │
└─────────────────┘
```

## Components

### 1. Core Types (`src/modules/autopolicy/mod.rs`)

#### LabelSet
A hashable, ordered set of labels that enables TrafficPattern to be used as a HashMap key.

```rust
pub struct LabelSet {
    labels: Vec<(String, String)>,
}

impl LabelSet {
    pub fn new(labels: HashMap<String, String>) -> Self
    pub fn to_hashmap(&self) -> HashMap<String, String>
    pub fn get(&self, key: &str) -> Option<&String>
    pub fn iter(&self) -> impl Iterator<Item = &(String, String)>
}
```

#### TrafficPattern
Represents a unique communication pattern between services:

```rust
pub struct TrafficPattern {
    pub src_namespace: String,
    pub src_labels: LabelSet,
    pub dst_namespace: String,
    pub dst_labels: LabelSet,
    pub port: u16,
    pub protocol: Protocol,
}
```

#### AutoPolicyConfig
Configuration for the learning engine:

```rust
pub struct AutoPolicyConfig {
    pub enabled: bool,
    pub learning_duration: Duration,        // e.g., 7 days
    pub min_observations: u64,              // e.g., 10
    pub auto_generate: bool,
    pub auto_apply: bool,                   // DANGEROUS!
    pub audit_mode: bool,                   // Safe default
    pub target_namespaces: Vec<String>,
    pub update_interval_secs: u64,
}
```

#### LearningState
Tracks the current phase:

```rust
pub enum LearningState {
    NotStarted,
    Learning { started_at: u64, progress: f32 },
    Completed { learned_at: u64 },
    Generating,
    Generated { count: usize },
    Applied { count: usize },
}
```

### 2. TrafficLearner (`src/modules/autopolicy/learner.rs`)

Observes traffic patterns over time:

```rust
pub struct TrafficLearner {
    observations: HashMap<TrafficPattern, TrafficObservation>,
    start_time: u64,
    min_observations: u64,
}

impl TrafficLearner {
    pub fn observe(&mut self, pattern: TrafficPattern, bytes: u64)
    pub fn significant_patterns(&self) -> Vec<&TrafficObservation>
    pub fn by_namespace(&self) -> HashMap<String, Vec<&TrafficObservation>>
    pub fn by_application(&self) -> HashMap<String, Vec<&TrafficObservation>>
    pub fn top_pairs(&self, limit: usize) -> Vec<&TrafficObservation>
}
```

**Features:**
- Incremental observation counting
- Byte transfer tracking
- First seen / last seen timestamps
- Filtering by minimum observations

### 3. PolicyGenerator (`src/modules/autopolicy/generator.rs`)

Generates CiliumNetworkPolicy YAML from learned patterns:

```rust
pub struct PolicyGenerator {
    min_observations: u64,
    audit_mode: bool,
}

impl PolicyGenerator {
    pub fn generate(
        &self,
        observations: &HashMap<TrafficPattern, TrafficObservation>,
    ) -> Vec<GeneratedPolicy>

    pub fn preview_policy(
        &self,
        namespace: &str,
        labels: &LabelSet,
        observations: &[&TrafficObservation],
    ) -> Result<String>
}
```

**Generated Policy Features:**
- Groups traffic by source (one policy per source app/namespace)
- Combines destinations into egress rules
- Groups ports by protocol (TCP/UDP)
- Adds metadata annotations (observation count, audit mode)
- Calculates confidence score (0.0 to 1.0)

**Confidence Scoring:**
- 40% based on observation count (more = better)
- 30% based on pattern consistency (fewer unique = better)
- 30% based on time span (longer observation = better)

### 4. TrafficAnalyzer (`src/modules/autopolicy/analyzer.rs`)

Provides insights on learned traffic:

```rust
pub struct TrafficAnalyzer;

impl TrafficAnalyzer {
    pub fn build_graph(
        observations: &HashMap<TrafficPattern, TrafficObservation>,
    ) -> CommunicationGraph

    pub fn find_isolated_services(...) -> Vec<String>
    pub fn find_hubs(..., limit: usize) -> Vec<(String, usize)>
    pub fn detect_security_issues(...) -> Vec<SecurityIssue>
    pub fn calculate_complexity(...) -> ComplexityScore
}
```

**Security Issue Detection:**
- Cross-namespace traffic
- Privileged ports (< 1024)
- Suspicious ports (SSH, RDP, IRC, etc.)

## Usage Example

```rust
use paqtra::modules::autopolicy::*;
use paqtra::ebpf::MockMapReader;
use paqtra::kubernetes::K8sClient;

#[tokio::main]
async fn main() -> Result<()> {
    // Configure learning
    let config = AutoPolicyConfig {
        enabled: true,
        learning_duration: Duration::from_secs(7 * 24 * 60 * 60), // 7 days
        min_observations: 10,
        auto_generate: true,
        auto_apply: false,      // Safe: don't auto-apply
        audit_mode: true,       // Safe: audit only
        target_namespaces: vec!["production".to_string()],
        update_interval_secs: 300,
    };

    // Create components
    let ebpf_reader = MockMapReader;  // Replace with real CiliumMapReader
    let k8s_client = K8sClient::new().await?;
    let mut autopolicy = AutoPolicy::new(config, ebpf_reader, k8s_client);

    // Start learning
    autopolicy.start_learning().await?;
    println!("Learning started...");

    // Update periodically
    loop {
        tokio::time::sleep(Duration::from_secs(300)).await;

        let stats = autopolicy.update().await?;
        println!("Observed {} connections, learned {} patterns",
            stats.connections_observed,
            stats.unique_patterns);

        // Check if learning is complete
        if let LearningState::Completed { .. } = autopolicy.state() {
            println!("Learning complete!");
            break;
        }
    }

    // Generate policies
    let policies = autopolicy.generate_policies()?;
    println!("Generated {} policies", policies.len());

    // Preview policies
    for policy in &policies {
        println!("\n--- Policy: {} (confidence: {:.0}%) ---",
            policy.name, policy.confidence * 100.0);
        println!("{}", policy.yaml);
    }

    // Optionally apply (manual approval recommended)
    // autopolicy.apply_policies().await?;

    Ok(())
}
```

## Example Generated Policy

```yaml
apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: auto-web-egress
  namespace: production
  labels:
    generated-by: paqtra
    autopolicy: "true"
  annotations:
    description: "Auto-generated from 142 traffic observations"
    audit-mode: "true"
spec:
  endpointSelector:
    matchLabels:
      app: "web"
  egress:
    - toEndpoints:
        - matchLabels:
            app: "api"
      toPorts:
        - ports:
            - port: "8080"
              protocol: TCP
    - toEndpoints:
        - matchLabels:
            app: "database"
      toPorts:
        - ports:
            - port: "5432"
              protocol: TCP
```

## Learning Workflow

### Phase 1: Learning (7 days default)

```
Day 1-7: Observe all traffic
↓
Record: src → dst patterns (namespace, labels, port, protocol)
↓
Count observations per pattern
↓
Track bytes transferred
↓
Update progress (0% → 100%)
```

### Phase 2: Analysis

```
Filter: Keep patterns with ≥ min_observations
↓
Group by source (namespace + labels)
↓
Group destinations per source
↓
Calculate confidence scores
```

### Phase 3: Generation

```
For each source:
  ↓
  Create CiliumNetworkPolicy
  ↓
  Add endpointSelector (source labels)
  ↓
  Add egress rules (destinations + ports)
  ↓
  Generate YAML
```

### Phase 4: Application (Optional)

```
Review generated policies
↓
Test in audit mode
↓
Apply to cluster (manual or auto)
```

## Safety Features

### Audit Mode (Recommended)
- Logs policy violations without blocking
- Safe to test generated policies
- Observe impact before enforcement

### Manual Review
- `auto_apply: false` (default for safety)
- Review YAML before applying
- Verify confidence scores

### Gradual Rollout
1. Start with audit mode
2. Test in non-production
3. Review violations for 1-2 weeks
4. Switch to enforce mode
5. Monitor for unexpected blocks

## Integration Points

### eBPF Data Source
```rust
pub trait MapReader {
    fn read_conntrack_map(&self) -> Result<Vec<ConntrackEntry>>;
}
```

Currently uses `MockMapReader` for testing. Production should use:
- Direct eBPF map reading via `libbpf`
- Hubble Relay gRPC API
- Cilium API

### Kubernetes Integration
```rust
impl K8sClient {
    pub async fn apply_custom_resource(&self, namespace: Option<&str>, yaml: &str) -> Result<()>
}
```

Applies generated CiliumNetworkPolicy to cluster.

### Policy Manager
```rust
impl PolicyManager {
    pub fn new(k8s_client: K8sClient) -> Self
}
```

Manages policy lifecycle.

## Testing

Run autopolicy tests:
```bash
cargo test autopolicy
```

Tests include:
- ✓ Protocol conversion
- ✓ AutoPolicy creation
- ✓ Learning phase start
- ✓ Learner creation and observation
- ✓ Generator policy name creation
- ✓ Analyzer suspicious port detection

## Future Enhancements

### v1.1 - Enhanced IP Resolution
- [ ] Integrate IPCache for IP → identity mapping
- [ ] Resolve pod labels from IPs
- [ ] Support external workloads

### v1.2 - Advanced Analysis
- [ ] Anomaly detection (unusual patterns)
- [ ] Baseline comparison
- [ ] Policy drift detection

### v1.3 - Multi-cluster
- [ ] Cross-cluster traffic learning
- [ ] Cluster mesh support
- [ ] Global policies

### v1.4 - ML-based Confidence
- [ ] Train on historical data
- [ ] Pattern clustering
- [ ] Predictive policy suggestions

## Performance Considerations

- **Memory:** O(unique patterns) - typically 100s to 1000s
- **CPU:** Minimal - updates every 5 minutes by default
- **Storage:** In-memory only (persist to disk for recovery)

For large clusters (>1000 pods):
- Increase `update_interval_secs` to reduce overhead
- Filter by `target_namespaces` to reduce scope
- Use higher `min_observations` to reduce noise

## Troubleshooting

### No patterns learned
- Check eBPF map access permissions
- Verify conntrack map is populated
- Ensure pods have labels

### Low confidence scores
- Increase learning duration
- Lower `min_observations` threshold
- Check for consistent traffic patterns

### Too many policies generated
- Increase `min_observations`
- Filter `target_namespaces`
- Use label selectors more broadly

## Related Modules

- **Self-Healer:** Detects and fixes policy gaps
- **Root-Cause Engine:** Explains policy drops
- **What-If Simulator:** Test policies before applying
- **Traffic Replay:** Replay historical traffic

## Status

✅ **Completed (v1.0)**
- Core learning engine
- Policy generation
- Traffic analysis
- Confidence scoring
- Tests passing

⏳ **In Progress**
- TUI integration
- Real eBPF map reading
- IPCache resolution

📋 **Planned**
- Advanced analytics
- ML-based improvements
- Multi-cluster support
