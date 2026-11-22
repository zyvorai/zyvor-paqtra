// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { AxiosError, AxiosResponse } from 'axios';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import * as api from '../../../services/api';
import CiliumInsights from '../index';

vi.mock('../../../services/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../services/api')>();
  return {
    ...actual,
    fetchCiliumFeatures: vi.fn(),
    fetchHubbleNodes: vi.fn(),
    fetchHubbleMetrics: vi.fn(),
    fetchCiliumMetrics: vi.fn(),
    fetchCiliumResources: vi.fn(),
    fetchAgentQuery: vi.fn(),
  };
});

const m = vi.mocked(api);
const ok = <T,>(data: T) => Promise.resolve({ data } as never);
const renderPage = () => render(<MemoryRouter><CiliumInsights /></MemoryRouter>);
const openTab = (name: RegExp) => fireEvent.click(screen.getByRole('button', { name }));

beforeEach(() => {
  vi.resetAllMocks();
  m.fetchCiliumFeatures.mockImplementation(() => ok({
    available: true,
    features: [
      { key: 'hubble', title: 'Hubble observability', category: 'Observability', state: 'enabled', view: '/flows' },
      { key: 'wireguard', title: 'WireGuard encryption', category: 'Encryption', state: 'disabled', view: '/wireguard' },
      { key: 'bgp', title: 'BGP control plane', category: 'Networking', state: 'unknown', view: '/bgp' },
      { key: 'policy-enforcement', title: 'Policy enforcement mode', category: 'Policy', state: 'set', value: 'default' },
    ],
  }));
});

describe('CiliumInsights', () => {
  it('shows each feature with its state, and links only features that exist', async () => {
    renderPage();
    expect(await screen.findByText('Hubble observability')).toBeInTheDocument();
    expect(screen.getByText('enabled')).toBeInTheDocument();
    expect(screen.getByText('disabled')).toBeInTheDocument();
    // (the intro sentence also says "unknown")
    expect(screen.getAllByText('unknown').length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText('default')).toBeInTheDocument();
    // Enabled and disabled features link to their page; an unknown one does not.
    expect(screen.getAllByRole('link', { name: 'Open' })).toHaveLength(2);
  });

  it('says so when the ConfigMap cannot be read', async () => {
    m.fetchCiliumFeatures.mockImplementation(() => ok({ available: false, reason: 'could not read the cilium-config ConfigMap', features: [] }));
    renderPage();
    expect(await screen.findByText(/could not read the cilium-config/)).toBeInTheDocument();
  });

  it('warns when a Hubble flow buffer is full', async () => {
    m.fetchHubbleNodes.mockImplementation(() => ok({
      available: true, total: 1, errors: [],
      nodes: [{ cluster: 'local', name: 'kind-worker', version: 'cilium v1.19', address: '', state: 'NODE_CONNECTED', tls_enabled: false, uptime_seconds: 7200, num_flows: 4095, max_flows: 4095, seen_flows: 999_999 }],
    }));
    renderPage();
    await screen.findByText('Hubble observability');
    openTab(/^Hubble$/);
    expect(await screen.findByText('kind-worker')).toBeInTheDocument();
    expect(screen.getByText(/buffer is full/)).toBeInTheDocument();
    expect(screen.getByText('connected')).toBeInTheDocument();
  });

  it('explains an unconfigured Prometheus instead of showing empty charts', async () => {
    const off = { available: false, reason: 'PROMETHEUS_URL is not set', metrics: [] };
    m.fetchHubbleMetrics.mockImplementation(() => ok(off));
    m.fetchCiliumMetrics.mockImplementation(() => ok(off));
    renderPage();
    await screen.findByText('Hubble observability');
    openTab(/Metrics/);
    expect(await screen.findAllByText(/PROMETHEUS_URL is not set/)).toHaveLength(2);
  });

  it('shows metric series, and the setting to change when a metric has no data', async () => {
    m.fetchHubbleMetrics.mockImplementation(() => ok({ available: true, metrics: [
      { key: 'drops_by_reason', title: 'Drops by reason', unit: 'drops/s', series: [{ labels: { reason: 'POLICY_DENIED', protocol: 'TCP' }, value: 1.5 }] },
      { key: 'dns_responses', title: 'DNS responses by rcode', unit: 'responses/s', series: [], hint: 'hubble.metrics.enabled=dns' },
    ] }));
    m.fetchCiliumMetrics.mockImplementation(() => ok({ available: true, metrics: [] }));
    renderPage();
    await screen.findByText('Hubble observability');
    openTab(/Metrics/);
    expect(await screen.findByText('POLICY_DENIED · TCP')).toBeInTheDocument();
    expect(screen.getByText(/hubble\.metrics\.enabled=dns/)).toBeInTheDocument();
  });

  it('marks a CRD that is not installed', async () => {
    m.fetchCiliumResources.mockImplementation(() => ok({ kind: 'nodes', installed: false, total: 0, items: [] }));
    renderPage();
    await screen.findByText('Hubble observability');
    openTab(/Resources/);
    expect(await screen.findByText(/CRD is not installed/)).toBeInTheDocument();
  });

  it('runs the chosen read-only agent query and shows its output', async () => {
    m.fetchAgentQuery.mockImplementation(() => ok({ what: 'policy-selectors', scope: 'one agent', data: [{ identities: [12836] }] }));
    renderPage();
    await screen.findByText('Hubble observability');
    openTab(/Agent/);
    fireEvent.change(screen.getByLabelText('Agent query'), { target: { value: 'policy-selectors' } });
    fireEvent.click(screen.getByRole('button', { name: 'Run' }));
    await waitFor(() => expect(m.fetchAgentQuery).toHaveBeenCalledWith('policy-selectors'));
    expect(await screen.findByText(/12836/)).toBeInTheDocument();
  });

  it('shows the server error when an agent query is refused', async () => {
    m.fetchAgentQuery.mockImplementation(() => Promise.reject(new AxiosError('x', '500', undefined, undefined, { status: 500, data: { error: 'no Cilium agent pod found' } } as AxiosResponse)));
    renderPage();
    await screen.findByText('Hubble observability');
    openTab(/Agent/);
    fireEvent.click(screen.getByRole('button', { name: 'Run' }));
    expect(await screen.findByText(/no Cilium agent pod found/)).toBeInTheDocument();
  });
});
