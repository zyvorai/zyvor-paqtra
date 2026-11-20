# Paqtra Web Application

Modern, cloud-native web interface for Paqtra network observability platform.

## Technology Stack

| Layer | Technology |
|-------|-----------|
| Framework | React 19 + TypeScript 5.8 |
| Build | Vite 6 with manual chunk splitting |
| Styling | Tailwind CSS 3.4 (dark-first, class-based theme) |
| State | Zustand 5 (auth, theme, preferences) |
| Data Fetching | TanStack React Query 5 + Axios |
| Charts | Recharts 2.15 |
| Icons | Lucide React |
| Routing | React Router 6 (lazy-loaded views) |
| Code Editor | Monaco Editor (YAML policy editing) |
| Graphs | D3-Force + D3-Selection + D3-Drag (tree-shaken) |
| Testing | Vitest 2 + React Testing Library |

## Project Structure

```
web-ui/
  src/
    App.tsx                    Router + providers (QueryClient, Toast, ErrorBoundary)
    main.tsx                   Entry point
    index.css                  Tailwind + custom utilities (gradients, glows, animations)
    components/                34 shared UI components
      MainLayout.tsx           Top navbar with hover dropdowns, mobile menu, WS status
      LoginPage.tsx            Auth form with remember-me, show/hide password
      StatCard.tsx             Stat card with gradient icon box, glow, trend
      HealthCheckCard.tsx      Health check result card
      ChartContainer.tsx       Reusable line/bar/pie chart wrapper
      SortableTable.tsx        Table with built-in column sorting
      ResponsiveTable.tsx      Desktop table + mobile cards
      PipelineView.tsx         Pipeline/workflow visualization
      SystemInfoPanel.tsx      System information display panel
      ActivityFeed.tsx         Real-time activity feed
      Badge.tsx                Status/severity/role badge (20+ variants)
      ProgressBar.tsx          Status-based progress bar with animations
      ScoreGauge.tsx           SVG circular gauge with score coloring
      Sparkline.tsx            Compact inline trend chart
      Accordion.tsx            Collapsible section with chevron
      AlertsList.tsx           Severity-colored alert cards with dismiss
      EmptyState.tsx           Styled empty state with icon + description
      ToggleSwitch.tsx         iOS-style toggle for boolean settings
      Toast.tsx                Context-based notification system (4 types)
      GlobalSearch.tsx         Command palette with keyboard navigation
      NamespaceSidebar.tsx     Namespace filtering sidebar
      Pagination.tsx           Paginated data navigation
      DataFreshness.tsx        Data staleness indicator with auto-refresh status
      FilterBar.tsx            Composable filter bar for data views
      BulkActionBar.tsx        Bulk selection action toolbar
      ExportButton.tsx         CSV/JSON export dropdown
      LoadingSpinner.tsx       CSS border spinner (3 sizes)
      LiveBadge.tsx            WS connection status badge/banner
      Skeleton.tsx             Shimmer loading placeholders
      Breadcrumbs.tsx          Route-based navigation breadcrumbs
      NotificationManager.tsx  Browser notification permission
    hooks/                     6 custom hooks
      useWebSocket.ts          Auto-reconnect WebSocket with JSON parsing
      useMetricsHistory.ts     Sliding-window metrics buffer
      useKeyboardShortcuts.ts  Global keyboard shortcuts (g-prefix navigation)
      usePageTitle.ts          Dynamic document.title
    stores/                    3 Zustand stores
      authStore.ts             Token auth with session check
      themeStore.ts            Dark/light toggle (class + localStorage)
      preferencesStore.ts      User preferences (page size, refresh, etc.)
    services/
      api.ts                   Axios client with 49 interfaces, 70+ API functions
    utils/
      formatters.ts            formatBytes, formatDuration, getStatusColor, etc.
      safe.ts                  Safe number handling (safeFixed, safePct, etc.)
    hooks/
      useAutoDismiss.ts        Auto-clearing state for success/error messages
      useKeyboardShortcuts.ts  Global keyboard shortcut handler
      useMetricsHistory.ts     Rolling metrics buffer
      usePageTitle.ts          Dynamic page title
      useWebSocket.ts          WebSocket connection with auto-reconnect
    views/                     64 view components (lazy-loaded)
```

## Views (64 pages)

| Category | Views |
|----------|-------|
| **Overview** | Dashboard, Events, Nodes, Endpoints, Host Info, Cluster Health, Cilium Status |
| **Observability** | Flows, Topology, Service Map, Heatmap, Dependencies, Latency, Flow Export, SLOs, DNS Monitor, Bandwidth, Interfaces, Metrics |
| **Security** | Policies, Templates, Policy Editor, Visual Rule Builder, Policy Rules, Cilium Insights, Pod Security, Anomalies, Security, Encryption, WireGuard, Identities, RBAC, Compliance, Audit Log, Alerts, Incidents, Change Log |
| **Intelligence** | AutoPolicy, Healer, Root Cause, Diagnostics, Troubleshoot, Forecasting |
| **Operations** | Chaos, Canary, Replay, Capture, Mirroring, MultiCluster, Cluster Mesh, BGP, Node Drain, eBPF |
| **eBPF Data** | Conntrack Table, Policy Map, IP Cache, LB Map, Drop Analytics |
| **Networking** | Load Balancer, Ingress, Egress GW, Service Mesh, KPR, IPAM, Cost Analytics, Settings |

## Design System

Dark-first UI design patterns:

- **Backgrounds**: `bg-slate-950` (page), `bg-slate-800/50` (cards), `bg-slate-900/50` (inputs/nested)
- **Borders**: `border-slate-700/50` (cards), `border-slate-700/30` (table rows)
- **Text**: `text-white` (headings), `text-slate-400` (secondary), `text-slate-500` (muted)
- **Gradients**: `text-gradient-blue`, stat-card-{blue,green,red,purple,orange,cyan}
- **Effects**: `card-glow`, `hover:scale-[1.02]`, `animate-fade-in`, `skeleton` shimmer
- **Navbar**: `navbar-gradient` with backdrop blur, hover dropdowns, WS connection dot
- **Buttons**: `bg-gradient-to-r from-blue-600 to-blue-700` (primary), border-based (secondary)
- **Tables**: `bg-slate-900/50` thead, `uppercase tracking-wider` headers, `table-row-hover`
- **Modals**: `modal-backdrop` with blur, `modal-card` with scale-in animation

## Development

```bash
cd web-ui
npm install
npm run dev          # Dev server on port 3000
npm run build        # Production build with tsc + vite
npm run test         # Vitest test suite
npm run lint         # ESLint (0 errors, 0 warnings)
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `/` | Open search |
| `?` | Show keyboard shortcuts |
| `r` | Refresh current view |
| `g h` | Go to Dashboard |
| `g f` | Go to Flows |
| `g t` | Go to Topology |
| `g p` | Go to Policies |
| `Esc` | Close modal/overlay |

## Build Output

- React vendor: 35 KB (gzip: 12 KB)
- Chart vendor: 406 KB (gzip: 111 KB)
- Icon vendor: 45 KB (gzip: 9 KB)
- App code: 267 KB (gzip: 84 KB)
- Sourcemaps included for debugging
