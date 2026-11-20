// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import * as api from '../../../services/api';
import Flows from '../index';

vi.mock('../../../services/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../services/api')>();
  return {
    ...actual,
    default: {
      ...actual.default,
      post: vi.fn(),
      get: vi.fn(),
    },
    fetchFlows: vi.fn(),
    fetchFlowStats: vi.fn(),
  };
});

const m = vi.mocked(api);
const ok = <T,>(data: T) => Promise.resolve({ data } as never);

const flow = (
  id: string,
  verdict = 'FORWARDED',
  port = 80,
): api.Flow => ({
  id,
  timestamp: '2026-09-24T03:30:00.000000Z',
  source: { namespace: 'shop', pod: 'web-1', ip: '10.0.0.1' },
  destination: { namespace: 'pay', pod: 'gw-1', ip: '10.0.0.2' },
  verdict,
  protocol: 'TCP',
  port,
});

const stats = (over: Partial<api.FlowStats> = {}): api.FlowStats => ({
  total_flows: 2,
  forwarded: 1,
  dropped: 1,
  requests_per_second: 0,
  avg_latency_ms: 0,
  ...over,
});

const renderPage = () =>
  render(
    <MemoryRouter>
      <Flows />
    </MemoryRouter>,
  );

beforeEach(() => {
  vi.resetAllMocks();
  m.fetchFlows.mockImplementation(() =>
    ok({ flows: [flow('f1'), flow('f2', 'DROPPED', 443)], total: 2 }),
  );
  m.fetchFlowStats.mockImplementation(() => ok(stats()));
  vi.mocked(m.default.post).mockResolvedValue({
    data: {
      id: 'explain-1',
      likely_owner: 'deny-egress',
      steps: [{ title: 'Policy drop', detail: 'CNP matched', confidence: 'observed' }],
      next_actions: ['Draft allow CNP'],
    },
  } as never);
});

describe('Flows: loading and rows', () => {
  it('shows waiting on Hubble when there are no flows', async () => {
    m.fetchFlows.mockImplementation(() => ok({ flows: [], total: 0 }));
    m.fetchFlowStats.mockImplementation(() =>
      ok(stats({ total_flows: 0, forwarded: 0, dropped: 0 })),
    );
    renderPage();
    expect(await screen.findByText(/No flows yet — waiting on Hubble Relay/)).toBeInTheDocument();
  });

  it('renders flow lines with verdict, endpoints, and port', async () => {
    renderPage();
    expect(
      await screen.findByText(/FORWARDED.*shop\/web-1 → pay\/gw-1:80/),
    ).toBeInTheDocument();
    expect(screen.getByText(/DROPPED.*shop\/web-1 → pay\/gw-1:443/)).toBeInTheDocument();
  });

  it('shows summary metrics from flow stats', async () => {
    renderPage();
    expect(await screen.findByText('flows sampled')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText('forwarded').nextSibling).toHaveTextContent('1'));
    expect(screen.getByText('dropped').nextSibling).toHaveTextContent('1');
    await waitFor(() => expect(screen.getByText('rows shown').nextSibling).toHaveTextContent('2'));
  });

  it('surfaces load errors in the warning card', async () => {
    m.fetchFlows.mockRejectedValue(new Error('hubble relay down'));
    renderPage();
    expect(await screen.findByText('hubble relay down')).toBeInTheDocument();
  });
});

describe('Flows: filters', () => {
  it('sends verdict and namespace on refresh, omits blanks', async () => {
    renderPage();
    await screen.findByText(/shop\/web-1 → pay\/gw-1:80/);
    expect(m.fetchFlows).toHaveBeenCalledWith({ limit: 100 });

    fireEvent.change(screen.getByPlaceholderText('FORWARDED'), { target: { value: 'DROPPED' } });
    fireEvent.change(screen.getByPlaceholderText('default'), { target: { value: 'shop' } });
    fireEvent.click(screen.getByRole('button', { name: /^Refresh$/ }));

    await waitFor(() =>
      expect(m.fetchFlows).toHaveBeenCalledWith({
        limit: 100,
        verdict: 'DROPPED',
        namespace: 'shop',
      }),
    );
  });
});

describe('Flows: Why denied?', () => {
  it('shows Why denied only on DROPPED rows', async () => {
    renderPage();
    expect(await screen.findByRole('button', { name: /Why denied\?/ })).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: /Why denied\?/ })).toHaveLength(1);
  });

  it('posts investigate/flow and renders the explain panel', async () => {
    renderPage();
    fireEvent.click(await screen.findByRole('button', { name: /Why denied\?/ }));

    await waitFor(() =>
      expect(m.default.post).toHaveBeenCalledWith('/investigate/flow', {
        flow_id: 'f2',
        flow: expect.objectContaining({ id: 'f2', verdict: 'DROPPED' }),
      }),
    );
    expect(await screen.findByText(/Owner: deny-egress/)).toBeInTheDocument();
    expect(screen.getByText('Policy drop')).toBeInTheDocument();
    expect(screen.getByText('Draft allow CNP')).toBeInTheDocument();
    expect(screen.getByText('WHY DENIED')).toBeInTheDocument();
  });
});
