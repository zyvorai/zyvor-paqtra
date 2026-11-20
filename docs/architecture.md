# Paqtra - Architecture Documentation

## Table of Contents
1. [High-Level Architecture](#high-level-architecture)
2. [Bootstrap Architecture](#bootstrap-architecture)
3. [Intelligence Layer](#intelligence-layer-deep-dive)
4. [Data Flow](#data-flow)
5. [Performance Characteristics](#performance-characteristics)
6. [Security Model](#security-model)
7. [Deployment Modes](#deployment-modes)

## High-Level Architecture

Paqtra is built as a layered architecture that separates concerns between data collection, intelligence processing, and user interface presentation.

```
┌─────────────────────────────────────────────────────────┐
│                  Terminal User Interface                │
│              (ratatui + crossterm)                      │
├─────────────────────────────────────────────────────────┤
│                   Intelligence Layer                    │
│  ┌──────────┬──────────┬──────────┬──────────────────┐ │
│  │ Packet   │ Auto     │ Root     │ Dry-run         │ │
│  │ Explainer│ Policy   │ Cause    │ Simulator       │ │
│  └──────────┴──────────┴──────────┴──────────────────┘ │
│  ┌──────────┬──────────┬──────────┬──────────────────┐ │
│  │ Replay   │ Chaos    │ Canary   │ Multi-cluster   │ │
│  │ Engine   │ Engine   │ Manager  │ Autopilot       │ │
│  └──────────┴──────────┴──────────┴──────────────────┘ │
├─────────────────────────────────────────────────────────┤
│                    Data Collection                      │
│  ┌──────────────┬────────────────┬──────────────────┐  │
│  │ Hubble gRPC  │ K8s API Client │ eBPF Maps Reader │  │
│  └──────────────┴────────────────┴──────────────────┘  │
├─────────────────────────────────────────────────────────┤
│                  Kubernetes & Cilium                    │
│  ┌──────────────┬────────────────┬──────────────────┐  │
│  │ Cilium Agent │ Hubble Relay   │ Kubernetes API   │  │
│  └──────────────┴────────────────┴──────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## Bootstrap Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         CILIUM TUI                              │
│                     (Zero-Touch Bootstrap)                      │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
        ┌────────────────────────────────────────┐
        │         Bootstrap Manager              │
        │  (Orchestrates entire setup flow)      │
        └────────────────────────────────────────┘
                              │
                ┌─────────────┼─────────────┐
                │             │             │
                ▼             ▼             ▼
        ┌───────────┐  ┌───────────┐  ┌──────────┐
        │ Kubernetes│  │  Cilium   │  │ Policies │
        │  Client   │  │  Manager  │  │ Manager  │
        └───────────┘  └───────────┘  └──────────┘
                │             │             │
                │             │             │
                ▼             ▼             ▼
        ┌─────────────────────────────────────────┐
        │       Kubernetes API Server             │
        └─────────────────────────────────────────┘
                              │
                ┌─────────────┼─────────────┐
                │             │             │
                ▼             ▼             ▼
        ┌───────────┐  ┌───────────┐  ┌──────────┐
        │ ConfigMap │  │  Policies │  │   RBAC   │
        │  (Hubble) │  │  (CNP)    │  │  (SA)    │
        └───────────┘  └───────────┘  └──────────┘
```

## Module Breakdown

### 1. Main Entry Point

```rust
main.rs
  │
  ├─→ Parse CLI args (clap)
  ├─→ Setup logging (tracing)
  ├─→ Create BootstrapManager
  ├─→ Run bootstrap (unless --skip-bootstrap)
  └─→ Launch TUI
```

### 2. Bootstrap Flow

```
BootstrapManager::run_bootstrap()
  │
  ├─→ detect_cluster()
  │   └─→ K8sClient::get_current_context()
  │
  ├─→ detect_cilium()
  │   └─→ CiliumManager::is_installed()
  │
  ├─→ enable_cilium_features()
  │   └─→ CiliumManager::enable_features()
  │       └─→ Create/update ConfigMap
  │
  ├─→ apply_default_policies()
  │   ├─→ PolicyManager::apply_intra_namespace_policy()
  │   ├─→ PolicyManager::apply_dns_policy()
  │   └─→ PolicyManager::apply_hubble_policy()
  │
  ├─→ setup_service_account()
  │   └─→ CiliumManager::create_tui_service_account()
  │       ├─→ Create ServiceAccount
  │       └─→ Create ClusterRoleBinding
  │
  └─→ setup_hubble_port_forward()
      └─→ Spawn background process
```

### 3. Kubernetes Client

```
K8sClient
  │
  ├─→ new() - Infer config from ~/.kube/config
  │
  ├─→ list_namespaces() - Get all namespaces
  ├─→ list_pods() - Get pods in namespace
  ├─→ get_pods_by_label() - Filter pods by label
  │
  ├─→ create_or_update_configmap() - Apply ConfigMap
  ├─→ create_or_update_service_account() - Apply SA
  ├─→ create_or_update_cluster_role_binding() - Apply CRB
  │
  └─→ apply_custom_resource() - Apply CRD (via kubectl)
```

### 4. Cilium Manager

```
CiliumManager
  │
  ├─→ is_installed()
  │   └─→ Check for pods with label k8s-app=cilium
  │
  ├─→ enable_features()
  │   └─→ Update cilium-config ConfigMap
  │       ├─→ enable-hubble: true
  │       ├─→ hubble-metrics-enabled: true
  │       └─→ enable-l7-proxy: true
  │
  └─→ create_tui_service_account()
      ├─→ Create ServiceAccount in kube-system
      └─→ Bind to cluster-admin role
```

### 5. Policy Manager

```
PolicyManager
  │
  ├─→ apply_intra_namespace_policy(ns)
  │   └─→ Allow all pods in same namespace to communicate
  │
  ├─→ apply_dns_policy(ns)
  │   └─→ Allow all pods to reach kube-dns
  │
  ├─→ apply_hubble_policy()
  │   └─→ Allow Hubble observability traffic
  │
  └─→ apply_best_practice_policy()
      └─→ Custom app-to-app policies
```

### 6. Hubble Client

```
HubbleClient
  │
  ├─→ new(port) - Create client for Hubble
  │
  ├─→ start_port_forward()
  │   └─→ Background: cilium hubble port-forward
  │
  └─→ get_flows()
      └─→ Fetch flows via Hubble Observer gRPC (CLI fallback when HUBBLE_MODE=cli|auto)
```

### 7. TUI Application

```
TuiApp
  │
  ├─→ new(context, port) - Initialize TUI
  │
  ├─→ run()
  │   ├─→ Setup terminal (crossterm)
  │   ├─→ Create backend (ratatui)
  │   ├─→ Event loop
  │   └─→ Cleanup on exit
  │
  ├─→ ui() - Main rendering
  │   ├─→ Title bar
  │   ├─→ Tab selector
  │   ├─→ Content area (based on selected tab)
  │   └─→ Footer (keyboard shortcuts)
  │
  └─→ Render functions
      ├─→ render_flows() - Live network traffic
      ├─→ render_endpoints() - Discovered endpoints
      ├─→ render_policies() - Active policies
      └─→ render_metrics() - Cluster metrics
```

## Data Flow

### Flow Monitoring

```
Cilium Agent
     │
     ▼
Hubble Observer
     │
     ▼
Port Forward (localhost:4245)
     │
     ▼
HubbleClient::get_flows()
     │
     ▼
Parse JSON flows
     │
     ▼
TuiApp.flows (Vec<Flow>)
     │
     ▼
render_flows()
     │
     ▼
Terminal Display
```

### Policy Creation

```
User runs paqtra
     │
     ▼
Bootstrap detects namespaces
     │
     ▼
For each namespace:
     │
     ├─→ Generate YAML for allow-intra-namespace
     ├─→ Generate YAML for allow-dns
     │
     ▼
K8sClient::apply_custom_resource()
     │
     ▼
kubectl apply -f -
     │
     ▼
Kubernetes API creates CiliumNetworkPolicy
     │
     ▼
Cilium Agent enforces policy
```

## State Management

### Application State

```rust
TuiApp {
    hubble_client: HubbleClient,      // Connection to Hubble
    flows: Vec<Flow>,                 // Latest flows
    selected_tab: usize,              // Current tab index
    context: String,                  // Cluster context
}
```

### Flow Data

```rust
Flow {
    time: String,                     // Timestamp
    verdict: String,                  // FORWARDED/DROPPED
    source: FlowEndpoint {            // Source pod
        namespace: String,
        pod_name: String,
        ip: String,
    },
    destination: FlowEndpoint {       // Dest pod
        namespace: String,
        pod_name: String,
        ip: String,
    },
    type: String,                     // L3/L4/L7
}
```

## Async Architecture

```
Tokio Runtime
  │
  ├─→ Main Thread
  │   └─→ TUI Event Loop (blocking)
  │
  ├─→ Background Task: Port Forward
  │   └─→ cilium hubble port-forward
  │
  └─→ Background Task: Flow Fetching
      └─→ Periodic HubbleClient::get_flows()
```

## Error Handling

```
Result<T, anyhow::Error>
  │
  ├─→ Bootstrap errors → Early exit with message
  ├─→ K8s API errors → Propagate with context
  ├─→ TUI errors → Cleanup terminal, then exit
  └─→ Flow fetch errors → Log, continue (non-fatal)
```

## Security Model

### Permissions Required

```
ClusterRole: cluster-admin
  │
  ├─→ Read all pods
  ├─→ Read all namespaces
  ├─→ Create/update ConfigMaps
  ├─→ Create/update CiliumNetworkPolicies
  ├─→ Create ServiceAccounts
  └─→ Create ClusterRoleBindings
```

### Resource Creation

All resources created are:
- Idempotent (can run multiple times)
- Non-destructive (don't delete existing resources)
- Namespace-scoped (except RBAC)
- Labeled for identification

## Performance Considerations

### Optimization Strategies

1. **Lazy Loading**
   - Only fetch flows when TUI is active
   - Limit flow history (last 100)

2. **Caching**
   - Cache namespace list
   - Cache pod labels

3. **Async Operations**
   - Port forward in background
   - Non-blocking K8s API calls

4. **Efficient Rendering**
   - Only redraw on data change or user input
   - Use ratatui's incremental rendering

## Extension Points

### Adding New Policy Types

```rust
impl PolicyManager {
    pub async fn apply_custom_policy(
        &self,
        namespace: &str,
        template: PolicyTemplate
    ) -> Result<()> {
        // Generate YAML from template
        // Apply via K8s client
    }
}
```

### Adding New TUI Tabs

```rust
impl TuiApp {
    fn render_my_new_tab(&self, f: &mut Frame, area: Rect) {
        // Custom rendering logic
    }
}
```

### Adding Metrics Collection

```rust
impl MetricsClient {
    pub async fn get_node_metrics(&self) -> Result<Vec<Metric>> {
        // Fetch from metrics-server or Prometheus
    }
}
```

## Deployment Modes

### Mode 1: Local CLI (Current)

```
Developer Laptop
  ├─→ paqtra binary
  ├─→ ~/.kube/config
  └─→ kubectl + cilium CLI
```

### Mode 2: In-Cluster (Future)

```
Kubernetes Pod
  ├─→ ServiceAccount (paqtra)
  ├─→ Direct K8s API access
  └─→ Direct Hubble gRPC connection
```

### Mode 3: Remote (Future)

```
Local Machine ←→ SSH/VPN ←→ Kubernetes Cluster
                               │
                               ├─→ TUI displays locally
                               └─→ Data fetched remotely
```

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_policy_generation() {
        // Test YAML generation
    }

    #[test]
    fn test_flow_parsing() {
        // Test JSON parsing
    }
}
```

### Integration Tests

```bash
# tests/integration_test.sh
minikube start
cilium install
cargo run --release
# Verify policies created
# Verify flows visible
```

## Debugging

### Log Levels

```bash
paqtra --verbose            # DEBUG level
paqtra                      # INFO level
RUST_LOG=trace paqtra       # TRACE level
```

### Common Issues

1. **Port forward fails**
   - Check: `ps aux | grep hubble`
   - Fix: `killall cilium && paqtra`

2. **No flows showing**
   - Check: `cilium hubble observe`
   - Fix: Verify Hubble enabled

3. **Permission denied**
   - Check: `kubectl auth can-i --list`
   - Fix: Use cluster-admin kubeconfig

---

## Intelligence Layer Deep Dive

### Data Collection Components

#### Hubble gRPC Client
- **Purpose**: Real-time network flow observation
- **Technology**: gRPC streaming, Protocol Buffers
- **Data**: Network flows with L3-L7 metadata
- **Performance**: Handles 100K+ flows/second

```rust
// Simplified flow collection
pub struct HubbleClient {
    client: ObserverClient<Channel>,
    flow_rx: mpsc::Receiver<Flow>,
}

impl HubbleClient {
    pub async fn observe_flows(&mut self) -> Result<Stream<Flow>> {
        let request = ObserveFlowsRequest {
            whitelist: vec![],
            blacklist: vec![],
            since: None,
            until: None,
        };

        let stream = self.client
            .observe_flows(request)
            .await?
            .into_inner();

        Ok(stream)
    }
}
```

#### eBPF Maps Reader
- **Purpose**: Identity maps, connection tracking
- **Technology**: Direct eBPF map reads via bpf() syscall
- **Data**: Endpoint identities, connection state
- **Access**: Read-only, no modifications

### Intelligence Modules

#### Packet Explainer Module
- **Purpose**: AI-like packet analysis with context
- **Input**: Network flow + metadata
- **Output**: What/Why/How/Security/Tips analysis
- **Features**:
  - Port-specific explanations (DNS, HTTP, databases)
  - Cross-namespace analysis
  - Security implication detection
  - Troubleshooting recommendations

**Algorithm**:
```
1. Extract flow metadata (source, dest, port, verdict)
2. Identify service type from port (HTTP, DNS, PostgreSQL, etc.)
3. Analyze verdict (FORWARDED, DROPPED, ERROR)
4. Check namespace boundary crossing
5. Query active policies for source/dest
6. Generate context-aware explanation
7. Provide actionable recommendations
```

#### AutoPolicy ML Confidence Module
- **Purpose**: ML-enhanced policy recommendations
- **Input**: Observed traffic patterns over time
- **Output**: CiliumNetworkPolicy + confidence score
- **Features**:
  - 7-feature confidence scoring
  - Temporal pattern analysis
  - Risk-based recommendations

**ML Scoring Algorithm**:
```rust
pub fn calculate_confidence(&self, pattern: &TrafficPattern) -> f64 {
    let temporal_stability = self.temporal_score(pattern);    // 20%
    let traffic_volume = self.volume_score(pattern);          // 20%
    let port_trust = self.port_trust_score(pattern);          // 15%
    let protocol_score = self.protocol_score(pattern);        // 10%
    let namespace_trust = self.namespace_trust(pattern);      // 15%
    let label_specificity = self.label_score(pattern);        // 10%
    let traffic_regularity = self.regularity_score(pattern);  // 10%

    temporal_stability * 0.20 +
    traffic_volume * 0.20 +
    port_trust * 0.15 +
    protocol_score * 0.10 +
    namespace_trust * 0.15 +
    label_specificity * 0.10 +
    traffic_regularity * 0.10
}
```

**Feature Breakdown**:

1. **Temporal Stability** (20%):
   - Measures pattern consistency over observation period
   - `score = min(observation_days / 7.0, 1.0)`
   - Longer observation = higher confidence

2. **Traffic Volume** (20%):
   - Logarithmic scaling of observation count
   - `score = min(log10(count + 1) / 3.0, 1.0)`
   - More observations = higher confidence

3. **Port Trust** (15%):
   - Well-known ports score higher
   - HTTP(80,443): 1.0, DNS(53): 1.0, Databases(3306,5432): 0.9
   - Unknown ports: 0.5

4. **Protocol Score** (10%):
   - TCP: 1.0 (reliable)
   - UDP: 0.7 (less reliable)
   - ICMP: 0.5 (diagnostic)

5. **Namespace Trust** (15%):
   - production: 1.0
   - kube-system: 0.95
   - staging: 0.8
   - default: 0.6

6. **Label Specificity** (10%):
   - More labels = more specific = higher confidence
   - `score = min(label_count / 5.0, 1.0)`

7. **Traffic Regularity** (10%):
   - Variance in traffic patterns
   - Low variance = high score

#### Chaos Engineering Engine
- **Purpose**: Controlled fault injection via eBPF
- **Input**: Experiment type, target, parameters
- **Output**: eBPF program + metrics
- **Safety**: Circuit breaker, auto-cleanup, limits

**Experiment Types**:
```rust
pub enum ChaosExperiment {
    PacketDrop { rate: f64, target: Target },
    LatencyInjection { delay_ms: u32, jitter_ms: u32, target: Target },
    BandwidthThrottle { limit_mbps: u32, target: Target },
    ConnectionTermination { kill_rate: f64, target: Target },
    DnsFailure { failure_rate: f64 },
    PacketCorruption { corruption_rate: f64 },
    PacketDuplication { dup_rate: f64 },
}
```

**Safety Mechanisms**:
1. **Rate Limiting**: Max 50% drop, 5000ms latency
2. **Circuit Breaker**: Emergency stop all experiments
3. **Auto-cleanup**: Remove the injected `tc netem` fault after its duration
4. **Confirmation**: Require user approval for high-severity
5. **Metrics**: Real-time impact tracking

#### Canary Manager
- **Purpose**: Progressive traffic shifting without sidecars
- **Technology**: Cilium L7 policies
- **Input**: Stable/canary deployments, traffic split
- **Output**: Updated policies, health metrics

**Decision Algorithm**:
```rust
pub fn evaluate_canary(&self, canary: &Canary) -> Recommendation {
    let success_rate = canary.metrics.success_rate;
    let error_rate = 1.0 - success_rate;
    let latency_ratio = canary.metrics.canary_latency / canary.metrics.stable_latency;

    if success_rate >= 0.99 && latency_ratio <= 1.2 && canary.current_split.canary_pct >= 90 {
        Recommendation::Promote
    } else if error_rate > 0.10 || latency_ratio > 2.0 {
        Recommendation::Rollback
    } else if success_rate >= 0.95 {
        Recommendation::Continue
    } else {
        Recommendation::Pause
    }
}
```

#### Multi-cluster Autopilot
- **Purpose**: Global orchestration and placement
- **Input**: Cluster metadata, workload requirements
- **Output**: Placement recommendations, policy syncs
- **Features**:
  - Cross-cluster topology discovery
  - Intelligent workload placement
  - Policy synchronization

**Placement Decision**:
```rust
pub fn recommend_placement(&self, workload: &Workload) -> PlacementRecommendation {
    let mut scores: HashMap<ClusterId, f64> = HashMap::new();

    for cluster in &self.clusters {
        let mut score = 0.0;

        // Resource availability (25%)
        score += self.resource_score(cluster, workload) * 0.25;

        // Network latency (20%)
        score += self.latency_score(cluster, workload) * 0.20;

        // Cost optimization (15%)
        score += self.cost_score(cluster) * 0.15;

        // Region affinity (15%)
        score += self.region_score(cluster, workload) * 0.15;

        // Load balancing (15%)
        score += self.load_balance_score(cluster) * 0.15;

        // Compliance (10%)
        score += self.compliance_score(cluster, workload) * 0.10;

        scores.insert(cluster.id, score);
    }

    // Return cluster with highest score
    let best_cluster = scores.iter().max_by_key(|(_, &score)| score);
    PlacementRecommendation {
        cluster_id: best_cluster.0.clone(),
        confidence: best_cluster.1,
        reasoning: self.generate_reasoning(workload, best_cluster.0),
    }
}
```

### Terminal User Interface

#### TUI Framework: Ratatui
- **Rendering**: Immediate mode UI with 60 FPS
- **Layout**: Constraint-based layout system
- **Widgets**: Custom widgets for each module

#### Event Loop
```rust
pub async fn run(&mut self) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let mut ticker = interval(Duration::from_millis(16)); // 60 FPS

    loop {
        tokio::select! {
            // UI rendering tick (60 FPS)
            _ = ticker.tick() => {
                terminal.draw(|f| self.render(f))?;
            }

            // Keyboard input
            Some(event) = self.event_rx.recv() => {
                if let Event::Key(key) = event {
                    if self.handle_key_event(key).await? {
                        break; // Quit
                    }
                }
            }

            // Data updates (flows, metrics)
            Some(flow) = self.flow_rx.recv() => {
                self.process_flow(flow);
            }
        }
    }

    Ok(())
}
```

#### View Architecture
- **Tab-based navigation**: 13 specialized views
- **Shared state**: Arc<RwLock<AppState>>
- **Reactive updates**: Flows → Views → Render

## Performance Characteristics

### Memory Usage
- **Base TUI**: ~20 MB
- **Flow buffer (10K flows)**: ~30 MB
- **Pattern learning**: ~20 MB
- **Total**: 50-100 MB typical

### CPU Usage
- **TUI rendering (60 FPS)**: ~5-10% single core
- **eBPF overhead**: <1% per core
- **Flow processing**: ~2-5% single core
- **Total**: 10-20% of one core typical

### Latency
- **UI responsiveness**: <16ms (60 FPS)
- **Policy evaluation**: Microseconds (kernel space)
- **Flow processing**: <1ms per flow
- **gRPC latency**: 10-50ms

### Scalability
- **Flow rate**: 100K flows/second supported
- **Concurrent connections**: 10K+ tracked
- **Policies**: 1000+ active policies
- **Clusters**: 10+ multi-cluster setups

## Advanced Security Model

### Read-Only by Default
- Hubble: Read-only flow observation
- eBPF maps: Read-only access
- Kubernetes: Read CRDs, policies

### Write Operations (Explicit)
- AutoPolicy: Write CiliumNetworkPolicy (with confirmation)
- RootCause: Apply policy fixes (with confirmation)
- Chaos: Inject `tc netem` faults via `kubectl exec` (with safety limits); Paqtra never attaches eBPF programs

### Audit Logging
All policy modifications logged:
```
2026-02-06T15:30:45Z [AUDIT] Applied policy: allow-frontend-backend
  Namespace: default
  Confidence: 95.2%
  User: admin
  Source: AutoPolicy ML
```

## Technology Stack

### Core Languages & Frameworks
- **Rust 1.70+**: Main language
- **Tokio**: Async runtime
- **Ratatui**: Terminal UI framework
- **Crossterm**: Terminal backend

### Kubernetes & Networking
- **kube-rs**: Kubernetes client
- **Cilium 1.14+**: Network dataplane
- **Hubble**: Observability API
- **gRPC**: Hubble communication

### Data Processing
- **Serde**: Serialization
- **Chrono**: Time handling
- **Regex**: Pattern matching

### Build & Development
- **Cargo**: Build system
- **Clippy**: Linting
- **rustfmt**: Code formatting

## What shipped after this document was first written

The REST API, the WebSocket flow stream, the web console, per-node agents, the
per-second metrics platform and anomaly detection are all part of the product
today; see [REST API](rest-api.md), [Metrics](metrics.md),
[Anomaly detection](anomaly-detection.md) and the
[changelog](https://github.com/zyvorai/zyvor-paqtra/blob/main/CHANGELOG.md).
Custom eBPF programs stay out of scope: Paqtra never attaches programs (see
[Cilium boundary](cilium-brotherhood.md)).

---

**Architecture Principles**:
1. **Separation of Concerns**: Data, Logic, Presentation layers
2. **Performance First**: <1% overhead, 60 FPS UI
3. **Safety by Default**: Confirmations, limits, circuit breakers
4. **Observable**: Metrics, logs, audit trails
5. **Extensible**: Plugin architecture for future growth
6. **Modular**: Easy to extend
7. **Testable**: Clear separation of concerns
8. **Maintainable**: Well-documented
9. **Performant**: Async-first design
