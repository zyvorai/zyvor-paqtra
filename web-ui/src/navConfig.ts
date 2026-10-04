import type { HeroTint } from './components/PageHero';

export type NavLink = { path: string; label: string; blurb: string };
export type NavGroup = { label: string; path?: string; children?: NavLink[] };

export type PageHeroCopy = {
  eyebrow: string;
  title: string;
  lede: string;
  tint?: HeroTint;
};

/**
 * Nav IA mirrors Netra 1:1 (Overview · Investigate · Diagnostics · Security · Reports · Fleet).
 * Labels/blurbs match Netra; routes map to Paqtra’s Cilium-native surfaces.
 */
export const NAV_GROUPS: NavGroup[] = [
  { label: 'Overview', path: '/' },
  {
    label: 'Investigate',
    children: [
      {
        path: '/investigate',
        label: 'Path',
        blurb: 'Why can’t A reach B — evidence-backed path diagnosis with confidence labels.',
      },
      {
        path: '/connectivity',
        label: 'Connectivity',
        blurb: 'Declare critical service paths and detect sustained flow regressions (observe-only).',
      },
      {
        path: '/flows',
        label: 'Flows',
        blurb: 'Hubble flows with verdict coloring — see where traffic goes and why it is allowed or dropped.',
      },
      {
        path: '/flows/history',
        label: 'History',
        blurb: 'Search stored flows over time: what was forwarded or dropped, and when.',
      },
      {
        path: '/endpoints',
        label: 'Endpoints',
        blurb: 'Cilium-managed endpoints and workload identities.',
      },
      {
        path: '/identities',
        label: 'Identities',
        blurb: 'Security identities resolved from the Cilium identity map.',
      },
      {
        path: '/dns',
        label: 'DNS',
        blurb: 'DNS queries observed by Cilium’s DNS proxy.',
      },
      {
        path: '/capture',
        label: 'Capture',
        blurb: 'Filtered, time-bounded packet capture for deep investigation.',
      },
      {
        path: '/topology',
        label: 'Topology',
        blurb: 'The observed-traffic dependency graph, live.',
      },
      {
        path: '/servicemap',
        label: 'Service Map',
        blurb: 'Service-to-service dependency graph from observed flows.',
      },
    ],
  },
  {
    label: 'Diagnostics',
    children: [
      {
        path: '/clusterhealth',
        label: 'Health',
        blurb: 'Cluster and Cilium health score — nodes, pods, endpoints, components.',
      },
      {
        path: '/node-metrics',
        label: 'Node Metrics',
        blurb: 'Per-second host, network, pod, Hubble and app metrics streamed by every agent.',
      },
      {
        path: '/metric-anomalies',
        label: 'Metric Anomalies',
        blurb: 'Unsupervised per-dimension anomaly scores; highlight a window to see what changed.',
      },
      {
        path: '/metric-alerts',
        label: 'Metric Alerts',
        blurb: 'Threshold and anomaly rules over the per-second metrics, with ack and silences.',
      },
      {
        path: '/latency',
        label: 'Path',
        blurb: 'Latency and path pressure across services.',
      },
      {
        path: '/drops',
        label: 'Drops',
        blurb: 'Packet drop analytics by Cilium reason — see also Root Cause.',
      },
      {
        path: '/rootcause',
        label: 'Root Cause',
        blurb: 'Correlate drops with policies and suggest fixes.',
      },
      {
        path: '/cilium-status',
        label: 'Cilium Status',
        blurb: 'Cilium agent and datapath status per node.',
      },
      {
        path: '/cilium-insights',
        label: 'Cilium Insights',
        blurb: 'Enabled Cilium features, Hubble node health, metrics and read-only agent views.',
      },
      {
        path: '/conntrack',
        label: 'Conntrack',
        blurb: 'eBPF connection tracking table (read-only).',
      },
      {
        path: '/ebpf',
        label: 'eBPF',
        blurb: 'Read-only Cilium program and map inventory — never mutates pins.',
      },
      {
        path: '/host',
        label: 'Host',
        blurb: 'Host and kernel information for the control plane.',
      },
    ],
  },
  {
    label: 'Security',
    children: [
      {
        path: '/incidents',
        label: 'Incidents',
        blurb: 'Incident timeline — signals joined by shared source.',
      },
      {
        path: '/anomalies',
        label: 'Anomalies',
        blurb: 'Detected network anomalies and review-only remediations.',
      },
      {
        path: '/policies',
        label: 'Policies',
        blurb: 'Plan and apply CiliumNetworkPolicy when CRDs are present.',
      },
      {
        path: '/policy-rules',
        label: 'Policy Rules',
        blurb: 'Add, edit or delete single rules of a CiliumNetworkPolicy — preview, then apply.',
      },
      {
        path: '/autopolicy',
        label: 'AutoPolicy',
        blurb: 'Generate Cilium policies from observed traffic — review before apply.',
      },
      {
        path: '/audit',
        label: 'Audit',
        blurb: 'Platform audit trail for policy and console actions.',
      },
      {
        path: '/compliance',
        label: 'Compliance',
        blurb: 'CIS, NIST, and compliance posture boards.',
      },
      {
        path: '/encryption',
        label: 'Encryption',
        blurb: 'Transparent encryption and WireGuard status.',
      },
    ],
  },
  {
    label: 'Reports',
    children: [
      {
        path: '/flows',
        label: 'Hubble',
        blurb: 'Optional Cilium enrichment when Hubble Relay is available.',
      },
      {
        path: '/metrics',
        label: 'Scorecard',
        blurb: 'API and platform metrics folded into an operator board.',
      },
      {
        path: '/bandwidth',
        label: 'Talkers',
        blurb: 'Top talkers by bandwidth — no payloads.',
      },
      {
        path: '/security',
        label: 'Security Posture',
        blurb: 'Zero-trust and security posture scoring.',
      },
      {
        path: '/users',
        label: 'Users',
        blurb: 'Accounts and roles: who can sign in and what they can change.',
      },
      {
        path: '/settings',
        label: 'Settings',
        blurb: 'Console preferences and connection settings.',
      },
    ],
  },
  { label: 'Fleet', path: '/nodes' },
];

/** Heroes match Netra pageHero copy, adapted for Cilium-native Paqtra. */
export const PAGE_HEROES: Record<string, PageHeroCopy> = {
  '/investigate': {
    eyebrow: 'Investigate',
    title: 'Why can’t A reach B?',
    lede: 'Evidence-backed path diagnosis — DNS, Service, identity, policy, node path, and changes — with observed / inferred / unavailable labels.',
    tint: 'green',
  },
  '/connectivity': {
    eyebrow: 'Investigate',
    title: 'Declared connectivity',
    lede: 'Monitor critical service paths. Quiet traffic stays unknown; sustained regressions alert with evidence — observe only.',
    tint: 'green',
  },
  '/flows/history': {
    eyebrow: 'Investigate',
    title: 'What happened, and when.',
    lede: 'Search stored flows over time: what was forwarded or dropped, and when.',
    tint: 'green',
  },
  '/flows': {
    eyebrow: 'Investigate',
    title: 'Follow every flow.',
    lede: 'Hubble flows with verdict coloring — see where traffic goes and why it is allowed or dropped.',
    tint: 'green',
  },
  '/endpoints': {
    eyebrow: 'Investigate',
    title: 'Know the endpoint.',
    lede: 'Cilium-managed endpoints and workload identities.',
  },
  '/identities': {
    eyebrow: 'Investigate',
    title: 'Know the identity.',
    lede: 'Security identities resolved from the Cilium identity map.',
  },
  '/dns': {
    eyebrow: 'Investigate',
    title: 'DNS as observed.',
    lede: 'DNS queries observed by Cilium’s DNS proxy.',
    tint: 'amber',
  },
  '/capture': {
    eyebrow: 'Investigate',
    title: 'Watch the wire, live.',
    lede: 'Filtered, time-bounded packet capture — observe-only.',
    tint: 'red',
  },
  '/topology': {
    eyebrow: 'Investigate',
    title: 'See the graph move.',
    lede: 'The observed-traffic dependency graph, live.',
    tint: 'purple',
  },
  '/servicemap': {
    eyebrow: 'Investigate',
    title: 'Service dependencies.',
    lede: 'Service-to-service dependency graph from observed flows.',
    tint: 'purple',
  },
  '/clusterhealth': {
    eyebrow: 'Diagnostics',
    title: 'TCP and cluster from the kernel.',
    lede: 'Cluster and Cilium health score — nodes, pods, endpoints, components.',
    tint: 'green',
  },
  '/node-metrics': {
    eyebrow: 'Diagnostics',
    title: 'Every node, every second.',
    lede: 'Host, network, pod, Hubble and application metrics, streamed live and kept in tiers.',
    tint: 'green',
  },
  '/metric-anomalies': {
    eyebrow: 'Diagnostics',
    title: 'What looks unusual.',
    lede: 'Per-dimension anomaly models; drag across the timeline to rank what changed.',
    tint: 'amber',
  },
  '/metric-alerts': {
    eyebrow: 'Diagnostics',
    title: 'Alerts from the metrics.',
    lede: 'Built-in and custom rules over per-second metrics. Alerts never apply policy.',
    tint: 'amber',
  },
  '/latency': {
    eyebrow: 'Diagnostics',
    title: 'Connect latency and pressure.',
    lede: 'Latency and path pressure across services.',
    tint: 'green',
  },
  '/drops': {
    eyebrow: 'Diagnostics',
    title: 'Where packets disappear.',
    lede: 'Packet drop analytics by Cilium reason, correlated with policy evidence.',
    tint: 'amber',
  },
  '/rootcause': {
    eyebrow: 'Diagnostics',
    title: 'Why it dropped.',
    lede: 'Correlate drops with policies and suggest fixes.',
    tint: 'amber',
  },
  '/cilium-status': {
    eyebrow: 'Diagnostics',
    title: 'Cilium on every node.',
    lede: 'Cilium agent and datapath status per node.',
    tint: 'green',
  },
  '/conntrack': {
    eyebrow: 'Diagnostics',
    title: 'Conntrack, read-only.',
    lede: 'eBPF connection tracking table — Paqtra never writes Cilium maps.',
  },
  '/ebpf': {
    eyebrow: 'Diagnostics',
    title: 'Observe everywhere.',
    lede: 'Read-only Cilium program and map inventory — never mutates pins.',
    tint: 'amber',
  },
  '/host': {
    eyebrow: 'Diagnostics',
    title: 'Host and kernel.',
    lede: 'Host and kernel information for the control plane.',
  },
  '/incidents': {
    eyebrow: 'Security',
    title: 'When signals agree.',
    lede: 'Incident timeline — signals joined by shared source.',
    tint: 'red',
  },
  '/anomalies': {
    eyebrow: 'Security',
    title: 'Behavior that stands out.',
    lede: 'Detected network anomalies and review-only remediations.',
    tint: 'purple',
  },
  '/policies': {
    eyebrow: 'Security',
    title: 'Cilium workbench.',
    lede: 'Plan and apply CiliumNetworkPolicy when CRDs are present.',
    tint: 'purple',
  },
  '/autopolicy': {
    eyebrow: 'Security',
    title: 'Learn, then review.',
    lede: 'Generate Cilium policies from observed traffic — review before apply.',
    tint: 'purple',
  },
  '/audit': {
    eyebrow: 'Security',
    title: 'What changed.',
    lede: 'Platform audit trail for policy and console actions.',
    tint: 'red',
  },
  '/compliance': {
    eyebrow: 'Security',
    title: 'Posture, checked.',
    lede: 'CIS, NIST, and compliance posture boards.',
    tint: 'amber',
  },
  '/encryption': {
    eyebrow: 'Security',
    title: 'Encryption on the wire.',
    lede: 'Transparent encryption and WireGuard status.',
  },
  '/metrics': {
    eyebrow: 'Reports',
    title: 'One board for the shift.',
    lede: 'API and platform metrics folded into an operator board.',
    tint: 'green',
  },
  '/bandwidth': {
    eyebrow: 'Reports',
    title: 'Who is talking the most.',
    lede: 'Top talkers by bandwidth — no payloads.',
    tint: 'amber',
  },
  '/security': {
    eyebrow: 'Reports',
    title: 'Security posture.',
    lede: 'Zero-trust and security posture scoring.',
    tint: 'purple',
  },
  '/users': {
    eyebrow: 'Reports',
    title: 'Who can do what.',
    lede: 'Accounts and roles: admins change things, viewers look.',
  },
  '/settings': {
    eyebrow: 'Reports',
    title: 'Settings.',
    lede: 'Console preferences and connection settings.',
  },
  '/nodes': {
    eyebrow: 'Fleet',
    title: 'Every node, one glance.',
    lede: 'Compact per-node inventory — the fleet board for this cluster.',
    tint: 'green',
  },
  '/bgp': {
    eyebrow: 'Diagnostics',
    title: 'BGP, peer by peer.',
    lede: 'BGP peer state and route advertisements from Cilium.',
  },
  '/clustermesh': {
    eyebrow: 'Fleet',
    title: 'ClusterMesh peers.',
    lede: 'Connected clusters and what they sync — endpoints and services.',
  },
  '/multicluster': {
    eyebrow: 'Fleet',
    title: 'Every cluster in view.',
    lede: 'Cross-cluster topology, health checks and policy sync.',
  },
  '/egress': {
    eyebrow: 'Diagnostics',
    title: 'Where traffic leaves.',
    lede: 'Egress gateway policies: gateway nodes, egress IPs and destinations.',
  },
  '/ingress': {
    eyebrow: 'Diagnostics',
    title: 'Where traffic enters.',
    lede: 'Ingress and Gateway API routes, hosts, paths and TLS.',
  },
  '/loadbalancer': {
    eyebrow: 'Diagnostics',
    title: 'Load-balanced services.',
    lede: 'Service frontends and their backends as Cilium balances them.',
  },
  '/lb-map': {
    eyebrow: 'Diagnostics',
    title: 'The LB map, read-only.',
    lede: 'Cilium load-balancer map entries — never written from this console.',
  },
  '/ipam': {
    eyebrow: 'Diagnostics',
    title: 'Addresses, allocated.',
    lede: 'IP pools and allocations handed out by Cilium IPAM.',
  },
  '/ipcache': {
    eyebrow: 'Diagnostics',
    title: 'The IP cache, read-only.',
    lede: 'IP to security identity mappings as the datapath sees them.',
  },
  '/policy-map': {
    eyebrow: 'Diagnostics',
    title: 'The policy map, read-only.',
    lede: 'Per-identity policy map entries with packet and byte counters.',
  },
  '/kpr': {
    eyebrow: 'Diagnostics',
    title: 'kube-proxy, replaced.',
    lede: 'Cilium eBPF kube-proxy replacement: mode, services and NAT tables.',
  },
  '/wireguard': {
    eyebrow: 'Security',
    title: 'WireGuard peers.',
    lede: 'Transparent-encryption peers, allowed IPs and handshakes.',
  },
  '/interfaces': {
    eyebrow: 'Diagnostics',
    title: 'Network interfaces.',
    lede: 'Per-node interfaces with byte, packet and error counters.',
  },
  '/service-mesh': {
    eyebrow: 'Diagnostics',
    title: 'The service mesh.',
    lede: 'Cilium service mesh configuration and traffic policies.',
  },
  '/dependencies': {
    eyebrow: 'Diagnostics',
    title: 'Who calls whom.',
    lede: 'Service dependencies observed from flows, with error rate and latency.',
  },
  '/flow-export': {
    eyebrow: 'Reports',
    title: 'Flows, exported.',
    lede: 'Configured flow exports and how much each has shipped.',
  },
  '/pod-security': {
    eyebrow: 'Security',
    title: 'Pod security.',
    lede: 'Pod Security Admission enforcement per namespace.',
    tint: 'purple',
  },
  '/rbac': {
    eyebrow: 'Security',
    title: 'Who can do what in the cluster.',
    lede: 'Kubernetes RBAC bindings: subjects, roles and permissions.',
    tint: 'purple',
  },
  '/alerts': {
    eyebrow: 'Reports',
    title: 'Alerts that matter.',
    lede: 'Alert rules, notification channels and silences.',
    tint: 'red',
  },
  '/events': {
    eyebrow: 'Reports',
    title: 'What just happened.',
    lede: 'Kubernetes cluster events, newest first.',
  },
  '/changelog': {
    eyebrow: 'Reports',
    title: 'What changed, when.',
    lede: 'Configuration change history with impact analysis.',
  },
  '/heatmap': {
    eyebrow: 'Reports',
    title: 'Traffic, as heat.',
    lede: 'Cross-namespace traffic density at a glance.',
    tint: 'amber',
  },
  '/slo': {
    eyebrow: 'Reports',
    title: 'Objectives, kept.',
    lede: 'Availability objectives, projected from the latest flow sample.',
    tint: 'green',
  },
  '/forecast': {
    eyebrow: 'Reports',
    title: 'What is coming.',
    lede: 'Resource forecasts with confidence intervals.',
  },
  '/costs': {
    eyebrow: 'Reports',
    title: 'What traffic costs.',
    lede: 'Network infrastructure cost breakdown by namespace.',
    tint: 'amber',
  },
  '/canary': {
    eyebrow: 'Diagnostics',
    title: 'Canary, measured.',
    lede: 'Progressive traffic shifting with auto-promote and auto-rollback.',
  },
  '/chaos': {
    eyebrow: 'Diagnostics',
    title: 'Chaos, on purpose.',
    lede: 'Network fault injection with tc-netem presets.',
    tint: 'red',
  },
  '/healer': {
    eyebrow: 'Diagnostics',
    title: 'Self-healing, supervised.',
    lede: 'Problem detection with proposed fixes.',
  },
  '/node-drain': {
    eyebrow: 'Fleet',
    title: 'Drain a node, safely.',
    lede: 'Gracefully drain nodes for maintenance.',
  },
  '/replay': {
    eyebrow: 'Investigate',
    title: 'Replay a moment.',
    lede: 'Record and replay network flows for time-travel debugging.',
  },
  '/mirror': {
    eyebrow: 'Diagnostics',
    title: 'Traffic, mirrored.',
    lede: 'Shadow traffic to debug a service without touching production.',
  },
  '/troubleshoot': {
    eyebrow: 'Diagnostics',
    title: 'Troubleshoot a path.',
    lede: 'End-to-end connectivity checks between two pods.',
  },
  '/diagnostics': {
    eyebrow: 'Diagnostics',
    title: 'Diagnostics.',
    lede: 'Connectivity tests, DNS checks and health validation.',
  },
  '/policy-editor': {
    eyebrow: 'Security',
    title: 'Write a policy.',
    lede: 'Write, validate and apply CiliumNetworkPolicies.',
    tint: 'purple',
  },
  '/rule-builder': {
    eyebrow: 'Security',
    title: 'Build a rule.',
    lede: 'Compose a policy rule from selectors and ports, then preview it.',
    tint: 'purple',
  },
  '/templates': {
    eyebrow: 'Security',
    title: 'Start from a template.',
    lede: 'Pre-built CiliumNetworkPolicy templates.',
    tint: 'purple',
  },
};

export function heroForPath(pathname: string): PageHeroCopy | null {
  if (PAGE_HEROES[pathname]) return PAGE_HEROES[pathname];
  for (const g of NAV_GROUPS) {
    for (const c of g.children ?? []) {
      if (c.path === pathname) {
        return {
          eyebrow: g.label,
          title: `${c.label}.`,
          lede: c.blurb,
        };
      }
    }
  }
  return {
    eyebrow: 'Paqtra',
    title: 'Trace every flow.',
    lede: 'Cilium-native observability — observe only; Cilium owns the datapath.',
  };
}
