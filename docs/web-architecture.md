# Paqtra Web Application Architecture

## Overview

Cloud-native web application providing a real-time dashboard for Cilium network observability, policy management, and intelligent automation. Built with React 19, TypeScript, and Tailwind CSS.

## Architecture Diagram

```
+-------------------------------------------------------------+
|                    Kubernetes Cluster                         |
+-------------------------------------------------------------+
|                                                               |
|  +---------------------------+  +--------------------------+ |
|  |   Frontend (React SPA)    |  |   Backend (Rust/Actix)   | |
|  |   Port: 3000 (dev)        |  |   Port: 9191             | |
|  |                            |  |                          | |
|  |  34 shared components      |  |  REST API (/api/v1/*)   | |
|  |  64 lazy-loaded views      |  |  WebSocket (/ws/*)      | |
|  |  Zustand state (3 stores)  |  |  JWT authentication     | |
|  |  WebSocket real-time       |  |  Hubble gRPC proxy      | |
|  +---------------------------+  +--------------------------+ |
|                                          |                    |
|                           +--------------+---------------+   |
|                           |                              |   |
|               +-----------v-----------+  +---------------v-+ |
|               |  Paqtra Core   |  |  Hubble Relay   | |
|               |  (Intelligence)       |  |  (Flow Data)    | |
|               |                       |  +-----------------+ |
|               |  AutoPolicy Engine    |                      |
|               |  Anomaly Detection    |                      |
|               |  Chaos Engineering    |                      |
|               |  eBPF Profiler        |                      |
|               +-----------------------+                      |
+-------------------------------------------------------------+
```

## Technology Stack

### Backend (Rust)
- **Framework**: Actix-Web
- **WebSocket**: actix-ws
- **Serialization**: serde_json
- **Authentication**: JWT (Bearer tokens)
- **Observability**: tracing, prometheus metrics

### Frontend (React)
- **Framework**: React 19 with TypeScript 5.6
- **State Management**: Zustand 5 (auth, theme, preferences)
- **Data Fetching**: TanStack React Query 5.56 + Axios
- **Routing**: React Router 6.28 with React.lazy code splitting
- **Visualization**: Recharts 2.15, D3 (d3-force, d3-selection, d3-drag)
- **Code Editor**: Monaco Editor (YAML policy editing)
- **Icons**: Lucide React
- **Styling**: Tailwind CSS 3.4 (dark-first, class-based toggle)
- **Build Tool**: Vite 6 (manual chunk splitting, sourcemaps)
- **Testing**: Vitest 2 + React Testing Library (68 tests)

## Frontend Architecture

### Component Layer (34 components)

```
components/
  Layout:       MainLayout, LoginPage
  Data Display: StatCard, HealthCheckCard, ChartContainer, SortableTable,
                ResponsiveTable, AlertsList, Badge, ProgressBar, ScoreGauge,
                Sparkline, PipelineView, SystemInfoPanel, ActivityFeed
  Feedback:     Toast, LoadingSpinner, Skeleton, EmptyState, LiveBadge,
                DataFreshness
  Navigation:   GlobalSearch, Breadcrumbs, NamespaceSidebar, Pagination
  Form:         ToggleSwitch, Accordion, ExportButton, NotificationManager,
                FilterBar, BulkActionBar
```

### State Management

| Store | Purpose | Persistence |
|-------|---------|-------------|
| authStore | Token, username, session check | localStorage + sessionStorage |
| themeStore | Dark/light toggle | localStorage |
| preferencesStore | Page size, refresh interval, etc. | localStorage |

### Hooks (6 hooks)

| Hook | Purpose |
|------|---------|
| useAutoRefresh | Configurable auto-refresh with interval control |
| useWebSocket | Auto-reconnect WS with JSON parsing |
| useMetricsHistory | Sliding-window buffer for time-series data |
| useKeyboardShortcuts | Global shortcuts with g-prefix navigation |
| useAutoDismiss | Auto-clearing state for success/error messages |
| usePageTitle | Dynamic document.title |

### View Layer (64 views)

All views are lazy-loaded via `React.lazy()` + `Suspense` for optimal code splitting.

Each view follows a consistent pattern:
1. Gradient icon box page header
2. Error/success alert banners
3. Stat cards with card-glow hover effects
4. Data tables with sortable headers, section headers, count badges
5. Charts with gradient icon headers and dark tooltip styling

### API Layer

Single Axios instance (`services/api.ts`) with:
- Auto-injected Bearer token from sessionStorage
- 401 response interceptor (clears auth state)
- 49 typed interfaces for all API responses
- 70+ typed API functions

## Design System

Dark-first theme patterns:

### Color Palette
- **Page**: `bg-slate-950` / `#0f172a`
- **Cards**: `bg-slate-800/50` with `border-slate-700/50`
- **Inputs**: `bg-slate-900/50` with `border-slate-700/50`
- **Primary text**: `text-white`
- **Secondary text**: `text-slate-400`
- **Muted text**: `text-slate-500`

### Stat Card System
Six gradient variants: `stat-card-{blue,green,red,purple,orange,cyan}`
Each with matching `card-glow-{color}` hover effect and `hover:scale-[1.02]`

### Component Patterns
- **Gradient icon box**: `w-10 h-10 rounded-lg bg-gradient-to-br from-{color}-500 to-{color}-700`
- **Section accent bar**: `w-1 h-5 bg-gradient-to-b from-{color}-400 to-{color}-500 rounded-full`
- **Terminal header**: Traffic light dots (red/yellow/green circles)
- **Table headers**: `uppercase tracking-wider font-semibold text-slate-400`
- **Buttons**: Gradient primary, border secondary, gradient-to-r reset

### Animations
- `animate-fade-in`: translateY(8px) + opacity
- `animate-scale-in`: scale(0.95) + opacity
- `animate-slide-in`: translateX(100%) + opacity
- `animate-pulse-dot`: opacity pulse for live indicators
- `skeleton`: shimmer gradient for loading states

### Light Theme
Full light theme support via `.light-theme` class + `html:not(.dark)` CSS variables.
Toggle persisted in localStorage.

## API Endpoints (75+ total)

### Health & Metrics
| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Health check |
| GET | `/ready` | Readiness probe |
| GET | `/metrics` | Prometheus metrics |

### Core Data (under /api/v1)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/flows` | Paginated flows with filters |
| GET | `/api/v1/flows/{id}` | Single flow detail |
| GET | `/api/v1/flows/stats` | Flow statistics |
| GET | `/api/v1/endpoints` | Cilium endpoints |
| GET | `/api/v1/nodes` | Node status and resources |
| GET | `/api/v1/events` | Kubernetes events |
| GET | `/api/v1/identities` | Cilium identities |

### Policies
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/policies` | List network policies |
| POST | `/api/v1/policies` | Create policy |
| PUT | `/api/v1/policies/{id}` | Update policy |
| DELETE | `/api/v1/policies/{id}` | Delete policy |
| POST | `/api/v1/policies/{id}/rules` | Add one rule (`direction`, `rule`; `?dry_run=true`) |
| PUT | `/api/v1/policies/{id}/rules` | Replace one rule (`direction`, `index`, `rule`) |
| DELETE | `/api/v1/policies/{id}/rules` | Delete one rule (`?direction=&index=`); a policy's last rule cannot be deleted (Cilium rejects rule-less policies), delete the policy instead |
| POST | `/api/v1/policies/simulate` | Simulate policy impact |
| POST | `/api/v1/policies/validate` | Validate YAML |
| GET | `/api/v1/policies/templates` | Policy templates |
| POST | `/api/v1/policies/templates/{id}/apply` | Apply template |

### Security
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/anomalies` | Detected anomalies |
| GET | `/api/v1/anomalies/{id}` | Anomaly detail |
| POST | `/api/v1/anomalies/{id}/remediate` | Remediate anomaly |
| GET | `/api/v1/compliance/frameworks` | Compliance frameworks |
| POST | `/api/v1/compliance/audit` | Run compliance audit |
| GET | `/api/v1/security/posture` | Security posture overview |
| GET | `/api/v1/security/findings` | Security findings |
| GET | `/api/v1/security/zero-trust` | Zero-trust score |
| GET | `/api/v1/security/pods` | Pod security status |

### Intelligence Modules
| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/v1/modules/autopolicy/generate` | ML policy generation |
| GET | `/api/v1/modules/chaos/experiments` | Chaos experiments |
| POST | `/api/v1/modules/chaos/run` | Run experiment |
| GET | `/api/v1/modules/canary/{id}` | Canary deployment status |
| GET | `/api/v1/modules/replay/recordings` | Flow recordings |
| POST | `/api/v1/modules/replay/start` | Start recording |
| POST | `/api/v1/modules/replay/{id}/stop` | Stop recording |
| GET | `/api/v1/modules/healer/problems` | Detected problems |
| POST | `/api/v1/modules/healer/{id}/fix` | Apply fix |
| GET | `/api/v1/modules/rootcause/drops` | Drop analysis |
| POST | `/api/v1/modules/rootcause/analyze` | Root cause analysis |
| GET | `/api/v1/modules/multicluster/clusters` | Multi-cluster status |
| POST | `/api/v1/modules/multicluster/{name}/sync` | Sync cluster |
| GET | `/api/v1/modules/ebpf/programs` | eBPF programs |
| GET | `/api/v1/modules/ebpf/maps` | eBPF maps |
| GET | `/api/v1/modules/capture/sessions` | Packet captures |
| POST | `/api/v1/modules/capture/start` | Start capture |
| POST | `/api/v1/modules/capture/{id}/stop` | Stop capture |
| GET | `/api/v1/modules/mirror/rules` | Mirror rules |
| POST | `/api/v1/modules/mirror/rules` | Create mirror rule |
| DELETE | `/api/v1/modules/mirror/rules/{id}` | Delete mirror rule |

### Observability
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/heatmap` | Traffic heatmap data |
| GET | `/api/v1/dependencies` | Service dependencies |
| GET | `/api/v1/servicemap` | Service map topology |
| GET | `/api/v1/dns/queries` | DNS query monitoring |
| GET | `/api/v1/dns/stats` | DNS statistics |
| GET | `/api/v1/latency/analysis` | Latency percentile analysis |
| GET | `/api/v1/bandwidth` | Bandwidth metrics |
| GET | `/api/v1/metrics/summary` | Metrics summary |

### Networking
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/network/interfaces` | Network interfaces |
| GET | `/api/v1/loadbalancer/services` | Load balancer services |
| GET | `/api/v1/ingress/routes` | Ingress routes |
| GET | `/api/v1/ipam/pools` | IPAM pools |
| GET | `/api/v1/ipam/allocations` | IP allocations |
| GET | `/api/v1/encryption/status` | Encryption status |
| GET | `/api/v1/wireguard/peers` | WireGuard peers |
| GET | `/api/v1/bgp/peers` | BGP peers |
| GET | `/api/v1/clustermesh/peers` | ClusterMesh peers |
| POST | `/api/v1/clustermesh/connect` | Connect cluster |
| GET | `/api/v1/kpr/status` | KPR status |
| GET | `/api/v1/egress/policies` | Egress policies |
| GET | `/api/v1/servicemesh/services` | Service mesh services |

### Operations
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/host/info` | Host information |
| GET | `/api/v1/cilium/status` | Cilium agent status |
| GET | `/api/v1/cilium/features` | Which Cilium features `cilium-config` enables |
| GET | `/api/v1/cilium/resources/{kind}` | List a Cilium CRD read-only (BGP v2, LB-IPAM, L2, pod IP pools, CIDR groups, Gateway API, ...) |
| GET | `/api/v1/cilium/metrics` | Cilium agent metrics via Prometheus |
| GET | `/api/v1/hubble/metrics` | Hubble flow metrics via Prometheus |
| GET | `/api/v1/hubble/nodes` | Hubble nodes with flow-buffer fill (`GetNodes`) |
| GET | `/api/v1/cilium/agent/{what}` | Read-only agent query: `endpoints`, `identities`, `services`, `fqdn-cache`, `status`, `policy-selectors` (editor) |
| GET | `/api/v1/cluster/health` | Cluster health |
| GET | `/api/v1/rbac/bindings` | RBAC bindings |
| GET | `/api/v1/alerts/rules` | Alert rules |
| POST | `/api/v1/alerts/rules` | Create alert rule |
| PUT | `/api/v1/alerts/rules/{id}/definition` | Edit alert rule name/condition/severity |
| DELETE | `/api/v1/alerts/rules/{id}` | Delete alert rule (seeded defaults need `?force=true`) |
| GET | `/api/v1/alerts/history` | Alert history |
| PUT | `/api/v1/alerts/rules/{id}` | Update alert rule |
| GET | `/api/v1/audit/log` | Audit log |
| GET | `/api/v1/slo/targets` | SLO targets |
| GET | `/api/v1/incidents` | Incidents |
| GET | `/api/v1/changes` | Change log |
| POST | `/api/v1/changes/{id}/rollback` | Rollback change |
| GET | `/api/v1/nodes/drain/status` | Node drain status |
| POST | `/api/v1/nodes/drain` | Drain node |
| POST | `/api/v1/nodes/uncordon` | Uncordon node |
| GET | `/api/v1/flows/exports` | Flow exports |
| POST | `/api/v1/flows/exports` | Create export |
| DELETE | `/api/v1/flows/exports/{id}` | Delete export |
| GET | `/api/v1/costs/breakdown` | Cost breakdown |
| GET | `/api/v1/forecast/metrics` | Forecast metrics |
| GET | `/api/v1/forecast/{metric}` | Forecast specific metric |
| POST | `/api/v1/diagnostics/run` | Run health checks |
| POST | `/api/v1/diagnostics/connectivity` | Connectivity test |
| POST | `/api/v1/troubleshoot/run` | Troubleshooting |

### eBPF Data (real kernel queries via bpftool)
| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/ebpf/programs` | List all eBPF programs (117 programs) |
| GET | `/api/v1/ebpf/maps` | List all eBPF maps (132 maps) |
| GET | `/api/v1/ebpf/conntrack` | Conntrack table entries (12,066 entries) |
| GET | `/api/v1/ebpf/conntrack/stats` | Conntrack statistics and summary |
| GET | `/api/v1/ebpf/policy-map` | Policy map entries per endpoint |
| GET | `/api/v1/ebpf/ipcache` | IP cache identity mappings (35 entries) |
| GET | `/api/v1/ebpf/lb` | Load balancer service/backend maps |
| GET | `/api/v1/ebpf/drops` | Drop reason categories (16 categories) |
| GET | `/api/v1/ebpf/drops/stats` | Drop statistics and trends |
| GET | `/api/v1/ebpf/map/{id}` | Dump specific eBPF map by ID |
| GET | `/api/v1/ebpf/profiler` | eBPF profiler with program/map metrics |

### WebSocket
| Method | Path | Description |
|--------|------|-------------|
| WS | `/api/v1/ws/flows` | Real-time flow stream |
| WS | `/api/v1/ws/metrics` | Real-time metrics stream |

## Development

```bash
# Frontend
cd web-ui
npm install
npm run dev        # Vite dev server (port 3000, proxies to :9191)
npm run build      # Production build (tsc + vite)
npm run test       # vitest
npm run lint       # ESLint (0 errors)

# Backend
cd web-api
cargo run          # API server on port 9191

# Full stack
docker-compose up
```

## Build & Performance

| Metric | Value |
|--------|-------|
| Production build | ~6s |
| Bundle size (gzip) | ~220 KB total |
| Code splitting | 4 vendor chunks + per-view lazy loading |
| TypeScript | Strict mode, 0 errors |
| ESLint | 0 errors, 0 warnings |
| Views | 64 lazy-loaded pages |
| Components | 34 shared UI components |
| API Endpoints | 75+ |
| Hooks | 6 custom hooks |

## Security

| Feature | Details |
|---------|---------|
| Authentication | JWT Bearer tokens |
| Authorization | RBAC with `require_admin` on destructive operations |
| Rate Limiting | Per-endpoint rate limiting |
| CORS | Configurable origin whitelist |
| WebSocket Limits | Max 100 concurrent connections |
| Input Validation | Request body validation on all POST/PUT endpoints |
