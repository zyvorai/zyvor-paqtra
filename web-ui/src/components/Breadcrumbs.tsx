// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';
import { useLocation, Link } from 'react-router-dom';
import { ChevronRight } from 'lucide-react';

const LABEL_MAP: Record<string, string> = {
  flows: 'Flows',
  topology: 'Topology',
  policies: 'Policies',
  anomalies: 'Anomalies',
  compliance: 'Compliance',
  autopolicy: 'AutoPolicy',
  chaos: 'Chaos',
  canary: 'Canary',
  settings: 'Settings',
  events: 'Events',
  nodes: 'Nodes',
  endpoints: 'Endpoints',
  host: 'Host Info',
  clusterhealth: 'Cluster Health',
  'cilium-status': 'Cilium Status',
  servicemap: 'Service Map',
  heatmap: 'Heatmap',
  dependencies: 'Dependencies',
  latency: 'Latency',
  'flow-export': 'Flow Export',
  slo: 'SLOs',
  dns: 'DNS Monitor',
  bandwidth: 'Bandwidth',
  interfaces: 'Interfaces',
  metrics: 'Metrics',
  templates: 'Templates',
  'policy-editor': 'Policy Editor',
  'rule-builder': 'Rule Builder',
  'policy-rules': 'Policy Rules',
  'cilium-insights': 'Cilium Insights',
  'pod-security': 'Pod Security',
  security: 'Security',
  encryption: 'Encryption',
  wireguard: 'WireGuard',
  identities: 'Identities',
  rbac: 'RBAC',
  audit: 'Audit Log',
  alerts: 'Alerts',
  incidents: 'Incidents',
  changelog: 'Change Log',
  healer: 'Healer',
  rootcause: 'Root Cause',
  diagnostics: 'Diagnostics',
  troubleshoot: 'Troubleshoot',
  forecast: 'Forecasting',
  replay: 'Replay',
  capture: 'Capture',
  mirror: 'Mirroring',
  multicluster: 'MultiCluster',
  clustermesh: 'Cluster Mesh',
  bgp: 'BGP Peering',
  'node-drain': 'Node Drain',
  ebpf: 'eBPF',
  loadbalancer: 'Load Balancer',
  ingress: 'Ingress',
  egress: 'Egress GW',
  'service-mesh': 'Service Mesh',
  kpr: 'KPR',
  ipam: 'IPAM',
  costs: 'Cost Analytics',
  conntrack: 'Conntrack Table',
  'policy-map': 'Policy Map',
  ipcache: 'IP Cache',
  'lb-map': 'LB Map',
  drops: 'Drop Analytics',
};

function segmentToLabel(segment: string): string {
  return LABEL_MAP[segment] || segment.replace(/-/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase());
}

const Breadcrumbs: React.FC = () => {
  const location = useLocation();
  const segments = location.pathname.split('/').filter(Boolean);

  if (segments.length === 0) return null;

  const crumbs: { label: string; path: string }[] = [
    { label: 'Home', path: '/' },
  ];

  let currentPath = '';
  for (const segment of segments) {
    currentPath += `/${segment}`;
    crumbs.push({ label: segmentToLabel(segment), path: currentPath });
  }

  return (
    <nav className="flex items-center gap-1 text-sm text-slate-400 mb-4" aria-label="Breadcrumb">
      {crumbs.map((crumb, index) => (
        <React.Fragment key={crumb.path}>
          {index > 0 && <ChevronRight className="w-3.5 h-3.5 text-slate-400/50 flex-shrink-0" />}
          {index < crumbs.length - 1 ? (
            <Link
              to={crumb.path}
              className="hover:text-white transition-colors"
            >
              {crumb.label}
            </Link>
          ) : (
            <span className="text-white font-medium">{crumb.label}</span>
          )}
        </React.Fragment>
      ))}
    </nav>
  );
};

export default Breadcrumbs;
