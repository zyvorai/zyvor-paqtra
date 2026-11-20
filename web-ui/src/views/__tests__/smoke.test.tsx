// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/**
 * Smoke render tests for Netra-parity Board pages.
 */
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

vi.mock('../../services/api', () => {
  const emptyGet = vi.fn().mockResolvedValue({ data: {} });
  const emptyPost = vi.fn().mockResolvedValue({ data: {} });
  const emptyPut = vi.fn().mockResolvedValue({ data: {} });
  const emptyDel = vi.fn().mockResolvedValue({ data: {} });
  const apiInstance = {
    get: emptyGet,
    post: emptyPost,
    put: emptyPut,
    delete: emptyDel,
    defaults: { headers: { common: {} } },
    interceptors: {
      request: { use: vi.fn(), handlers: [] },
      response: { use: vi.fn(), handlers: [] },
    },
  };
  return {
    default: apiInstance,
    fetchFlows: vi.fn().mockResolvedValue({ data: { flows: [], total: 0 } }),
    fetchFlowStats: vi.fn().mockResolvedValue({ data: { total_flows: 0, forwarded: 0, dropped: 0 } }),
    fetchPolicies: vi.fn().mockResolvedValue({ data: { policies: [] } }),
    generateAutopolicy: emptyPost,
    fetchNodes: vi.fn().mockResolvedValue({ data: { nodes: [] } }),
    fetchEndpoints: vi.fn().mockResolvedValue({ data: { endpoints: [] } }),
    fetchAnomalies: vi.fn().mockResolvedValue({ data: { anomalies: [] } }),
    fetchFrameworks: vi.fn().mockResolvedValue({ data: { frameworks: [] } }),
    fetchZeroTrustScore: vi.fn().mockResolvedValue({ data: { overall: 0 } }),
    fetchSecurityFindings: vi.fn().mockResolvedValue({ data: { findings: [] } }),
    fetchClusterHealth: vi.fn().mockResolvedValue({ data: { status: 'healthy', score: 100, components: [] } }),
    fetchServiceMap: vi.fn().mockResolvedValue({ data: { nodes: [], edges: [] } }),
    fetchServiceDeps: vi.fn().mockResolvedValue({ data: { dependencies: [] } }),
    fetchPacketDrops: vi.fn().mockResolvedValue({ data: { drops: [] } }),
    fetchCiliumStatus: vi.fn().mockResolvedValue({ data: { agents: [] } }),
    fetchEbpfSummary: vi.fn().mockResolvedValue({ data: {} }),
    fetchEbpfDrops: vi.fn().mockResolvedValue({ data: { drops: [], total_drops: 0 } }),
    fetchIdentities: vi.fn().mockResolvedValue({ data: { identities: [] } }),
    fetchAuditLog: vi.fn().mockResolvedValue({ data: { entries: [] } }),
    fetchMetricsSummary: vi.fn().mockResolvedValue({ data: {} }),
    fetchDnsQueries: vi.fn().mockResolvedValue({ data: { queries: [] } }),
    fetchDnsStats: vi.fn().mockResolvedValue({ data: {} }),
    fetchCaptureSessions: vi.fn().mockResolvedValue({ data: { sessions: [] } }),
    fetchLatencyAnalysis: vi.fn().mockResolvedValue({ data: { services: [] } }),
    fetchEbpfConntrack: vi.fn().mockResolvedValue({ data: { entries: [], total: 0 } }),
    fetchRealEbpfPrograms: vi.fn().mockResolvedValue({ data: { programs: [], total: 0 } }),
    fetchRealEbpfMaps: vi.fn().mockResolvedValue({ data: { maps: [], total: 0 } }),
    fetchHostInfo: vi.fn().mockResolvedValue({ data: {} }),
    fetchIncidents: vi.fn().mockResolvedValue({ data: { incidents: [] } }),
    fetchEncryptionStatus: vi.fn().mockResolvedValue({ data: {} }),
    fetchWireGuardPeers: vi.fn().mockResolvedValue({ data: { peers: [] } }),
    fetchBandwidthData: vi.fn().mockResolvedValue({ data: { entries: [] } }),
    checkHealth: emptyGet,
    UNAUTHORIZED_EVENT: 'paqtra:unauthorized',
    fetchMe: vi.fn().mockResolvedValue({ data: { username: 'admin', role: 'admin', source: 'config' } }),
    changePassword: emptyPost,
    apiErrorMessage: (_e: unknown, fallback: string) => fallback,
  };
});

function renderView(View: React.ComponentType) {
  return render(
    <MemoryRouter>
      <View />
    </MemoryRouter>,
  );
}

beforeEach(() => vi.clearAllMocks());
afterEach(() => vi.clearAllMocks());

const cases: { name: string; path: string; needle: RegExp }[] = [
  { name: 'Dashboard', path: '../Dashboard', needle: /Brothers with Cilium|ON-CALL DIGEST/i },
  { name: 'Flows', path: '../Flows', needle: /LIVE STREAM|hubble\.GetFlows/i },
  { name: 'InvestigatePath', path: '../InvestigatePath', needle: /INVESTIGATE|Why can.?t A reach B/i },
  { name: 'Topology', path: '../Topology', needle: /TOPOLOGY/i },
  { name: 'ServiceMap', path: '../ServiceMap', needle: /SERVICE MAP/i },
  { name: 'DnsMonitor', path: '../DnsMonitor', needle: /DNS PULSE/i },
  { name: 'PacketCapture', path: '../PacketCapture', needle: /CAPTURE/i },
  { name: 'ClusterHealth', path: '../ClusterHealth', needle: /HEALTH PULSE/i },
  { name: 'DropDashboard', path: '../DropDashboard', needle: /DROP PULSE/i },
  { name: 'RootCause', path: '../RootCause', needle: /ROOT CAUSE/i },
  { name: 'Policies', path: '../Policies', needle: /Cilium workbench/i },
  { name: 'Anomalies', path: '../Anomalies', needle: /ANOMALIES/i },
  { name: 'AuditLog', path: '../AuditLog', needle: /audit trail/i },
  { name: 'Nodes', path: '../Nodes', needle: /Every node/i },
  { name: 'MetricsDash', path: '../MetricsDash', needle: /SCORECARD/i },
  { name: 'SecurityDash', path: '../SecurityDash', needle: /SECURITY POSTURE/i },
  { name: 'Settings', path: '../Settings', needle: /CONNECTION|MORE/i },
];

describe.each(cases)('$name view', ({ path, needle }) => {
  it('renders Netra board chrome', async () => {
    const mod = await import(/* @vite-ignore */ path);
    renderView(mod.default);
    expect(screen.getAllByText(needle).length).toBeGreaterThan(0);
  });
});
