// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { render, screen, waitFor, fireEvent, within, act } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { AxiosError, AxiosResponse } from 'axios';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import * as api from '../../../services/api';
import FlowHistory from '../index';

vi.mock('../../../services/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../services/api')>();
  return { ...actual, fetchFlowHistory: vi.fn(), fetchFlowTimeline: vi.fn() };
});

const m = vi.mocked(api);
const ok = <T,>(data: T) => Promise.resolve({ data } as never);
const httpError = (status: number, error: string) =>
  Promise.reject(new AxiosError('x', String(status), undefined, undefined, { status, data: { error } } as AxiosResponse));

const COVERAGE = { oldest: '2026-09-20T00:00:00.000000Z', newest: '2026-09-24T04:00:00.000000Z', stored_flows: 5000, retention_days: 7, durable: true, range_starts_before_oldest: false };
const CAPTURE = { interval_secs: 30, batch: 500, last_capture_ok: true, last_capture_at: '2026-09-24T04:00:00.000000Z', note: 'Flows are captured by polling Hubble every 30s and keeping the most recent 500 it returns; counts are lower bounds.' };
const RANGE = { from: '2026-09-24T03:00:00.000000Z', to: '2026-09-24T04:00:00.000000Z' };

const flow = (id: string, verdict = 'FORWARDED', port = 80) => ({
  id, timestamp: '2026-09-24T03:30:00.000000Z', cluster: 'local', verdict, drop_reason: '', protocol: 'TCP', port,
  source: { namespace: 'shop', pod: 'web-1', ip: '10.0.0.1' }, destination: { namespace: 'pay', pod: 'gw-1', ip: '10.0.0.2' },
});
const history = (over: Partial<api.FlowHistoryResponse> = {}): api.FlowHistoryResponse => ({
  flows: [flow('f1'), flow('f2', 'DROPPED', 443)], total: 2, limit: 50, offset: 0, range: RANGE, coverage: COVERAGE, capture: CAPTURE, ...over,
});
const timeline = (over: Partial<api.FlowTimelineResponse> = {}): api.FlowTimelineResponse => ({
  bucket_secs: 60, total: 2, range: RANGE, coverage: COVERAGE, capture: CAPTURE,
  buckets: [
    { start: '2026-09-24T03:29:00.000000Z', forwarded: 1, dropped: 0, other: 0 },
    { start: '2026-09-24T03:30:00.000000Z', forwarded: 0, dropped: 1, other: 0 },
  ], ...over,
});

const renderPage = () => render(<MemoryRouter><FlowHistory /></MemoryRouter>);
const search = () => fireEvent.click(screen.getByRole('button', { name: /^Search/ }));
const timelineParams = () => m.fetchFlowTimeline.mock.calls.at(-1)![0];
const historyParams = () => m.fetchFlowHistory.mock.calls.at(-1)![0];

beforeEach(() => {
  vi.resetAllMocks();
  m.fetchFlowTimeline.mockImplementation(() => ok(timeline()));
  m.fetchFlowHistory.mockImplementation(() => ok(history()));
});

describe('FlowHistory: loading and results', () => {
  it('loads the last hour on open, for the chart and the table with the same range', async () => {
    const before = Date.now();
    renderPage();
    expect(await screen.findByText('2')).toBeInTheDocument(); // flows found
    const t = timelineParams(); const h = historyParams();
    expect(t.from).toBe(h.from);
    expect(t.to).toBe(h.to);
    const span = new Date(t.to!).getTime() - new Date(t.from!).getTime();
    expect(span).toBe(60 * 60_000);
    expect(new Date(t.to!).getTime()).toBeGreaterThanOrEqual(before);
    expect(h.limit).toBe(50);
    expect(h.offset).toBe(0);
    expect(Object.keys(t)).not.toContain('limit');
  });

  it('shows the KPIs, the chart and each flow with both endpoints', async () => {
    renderPage();
    expect(await screen.findByText('flows found')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText('dropped').nextSibling).toHaveTextContent('1'));
    expect(screen.getByText('per bar').nextSibling).toHaveTextContent('1 min');
    expect(screen.getByRole('list', { name: 'Legend' })).toBeInTheDocument();
    const rows = within(screen.getByRole('columnheader', { name: 'Verdict' }).closest('table')!).getAllByRole('row');
    expect(rows).toHaveLength(3);
    expect(within(rows[1]).getByText('shop/web-1')).toBeInTheDocument();
    expect(within(rows[1]).getByText('pay/gw-1')).toBeInTheDocument();
    expect(within(rows[2]).getByText('DROPPED')).toBeInTheDocument();
    expect(within(rows[2]).getByText('TCP 443')).toBeInTheDocument();
  });

  it('falls back to the IP when a flow has no pod, and omits an empty namespace', async () => {
    m.fetchFlowHistory.mockImplementation(() => ok(history({ flows: [{ ...flow('w'), source: { namespace: '', pod: '', ip: '203.0.113.9' } }], total: 1 })));
    renderPage();
    expect(await screen.findByText('203.0.113.9')).toBeInTheDocument();
  });

  it('says when nothing matches, and draws neither chart nor table', async () => {
    m.fetchFlowTimeline.mockImplementation(() => ok(timeline({ total: 0, buckets: [{ start: RANGE.from, forwarded: 0, dropped: 0, other: 0 }] })));
    m.fetchFlowHistory.mockImplementation(() => ok(history({ flows: [], total: 0 })));
    renderPage();
    expect(await screen.findByText(/No stored flows match this range/)).toBeInTheDocument();
    expect(screen.queryByRole('list', { name: 'Legend' })).not.toBeInTheDocument();
    expect(screen.queryByRole('columnheader', { name: 'Verdict' })).not.toBeInTheDocument();
  });

  it('shows the server reason when a search fails', async () => {
    m.fetchFlowTimeline.mockImplementation(() => httpError(400, 'the range can cover at most 30 days'));
    renderPage();
    expect(await screen.findByText('the range can cover at most 30 days')).toBeInTheDocument();
  });
});

describe('FlowHistory: filters', () => {
  it('sends the filters, trims text, uppercases via the verdict list, and omits blanks', async () => {
    renderPage();
    await screen.findByText('flows found');
    fireEvent.change(screen.getByLabelText('Namespace'), { target: { value: '  shop ' } });
    fireEvent.change(screen.getByLabelText('Pod'), { target: { value: 'web' } });
    fireEvent.change(screen.getByLabelText('Port'), { target: { value: '443' } });
    fireEvent.change(screen.getByLabelText('Verdict'), { target: { value: 'DROPPED' } });
    fireEvent.change(screen.getByLabelText('Range'), { target: { value: '24h' } });
    search();
    await waitFor(() => expect(m.fetchFlowTimeline).toHaveBeenCalledTimes(2));
    const t = timelineParams();
    expect(t).toMatchObject({ namespace: 'shop', pod: 'web', port: 443, verdict: 'DROPPED' });
    expect(new Date(t.to!).getTime() - new Date(t.from!).getTime()).toBe(24 * 3600_000);
    expect(historyParams()).toMatchObject({ namespace: 'shop', pod: 'web', port: 443, verdict: 'DROPPED', offset: 0 });
  });

  it('leaves out filters that are blank', async () => {
    renderPage();
    await screen.findByText('flows found');
    const t = timelineParams();
    expect(t.namespace).toBeUndefined();
    expect(t.pod).toBeUndefined();
    expect(t.port).toBeUndefined();
    expect(t.verdict).toBeUndefined();
  });

  it('does not fetch until Search is pressed', async () => {
    renderPage();
    await screen.findByText('flows found');
    fireEvent.change(screen.getByLabelText('Namespace'), { target: { value: 'shop' } });
    fireEvent.change(screen.getByLabelText('Pod'), { target: { value: 'x' } });
    expect(m.fetchFlowTimeline).toHaveBeenCalledTimes(1);
  });

  it('rejects a bad port before asking the server', async () => {
    renderPage();
    await screen.findByText('flows found');
    for (const bad of ['0', '70000', 'abc', '1.5']) {
      fireEvent.change(screen.getByLabelText('Port'), { target: { value: bad } });
      search();
      expect(await screen.findByText('Port must be a number from 1 to 65535.')).toBeInTheDocument();
    }
    expect(m.fetchFlowTimeline).toHaveBeenCalledTimes(1);
  });

  it('takes a custom range as local times and sends it as ISO', async () => {
    renderPage();
    await screen.findByText('flows found');
    fireEvent.change(screen.getByLabelText('Range'), { target: { value: 'custom' } });
    fireEvent.change(screen.getByLabelText('From'), { target: { value: '2026-09-24T02:00' } });
    fireEvent.change(screen.getByLabelText('To'), { target: { value: '2026-09-24T03:30' } });
    search();
    await waitFor(() => expect(m.fetchFlowTimeline).toHaveBeenCalledTimes(2));
    expect(timelineParams()).toMatchObject({ from: new Date('2026-09-24T02:00').toISOString(), to: new Date('2026-09-24T03:30').toISOString() });
  });

  it('explains an invalid custom range without calling the server', async () => {
    renderPage();
    await screen.findByText('flows found');
    fireEvent.change(screen.getByLabelText('Range'), { target: { value: 'custom' } });
    search();
    expect(await screen.findByText('Choose both a start and an end time.')).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText('From'), { target: { value: '2026-09-24T05:00' } });
    fireEvent.change(screen.getByLabelText('To'), { target: { value: '2026-09-24T04:00' } });
    search();
    expect(await screen.findByText('The start must be earlier than the end.')).toBeInTheDocument();
    expect(m.fetchFlowTimeline).toHaveBeenCalledTimes(1);
  });
});

describe('FlowHistory: paging', () => {
  it('pages with the same range and filters as the chart, never re-reading "now"', async () => {
    m.fetchFlowHistory.mockImplementation((p) => ok(history({ total: 120, offset: p.offset ?? 0, flows: [flow(`f${p.offset}`)] })));
    renderPage();
    expect(await screen.findByText('1–1 of 120')).toBeInTheDocument();
    const first = historyParams();
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    await waitFor(() => expect(m.fetchFlowHistory).toHaveBeenCalledTimes(2));
    const second = historyParams();
    expect(second.offset).toBe(50);
    expect(second.from).toBe(first.from);
    expect(second.to).toBe(first.to);
    expect(m.fetchFlowTimeline).toHaveBeenCalledTimes(1); // the chart is not refetched
  });

  it('disables Previous on the first page and Next on the last', async () => {
    m.fetchFlowHistory.mockImplementation((p) => ok(history({ total: 60, offset: p.offset ?? 0, flows: Array.from({ length: (p.offset ?? 0) === 0 ? 50 : 10 }, (_, i) => flow(`f${p.offset}-${i}`)) })));
    renderPage();
    await screen.findByText('1–50 of 60');
    expect(screen.getByRole('button', { name: 'Previous' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    await screen.findByText('51–60 of 60');
    expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Previous' }));
    await waitFor(() => expect(historyParams().offset).toBe(0));
  });

  it('separates thousands in the counts', async () => {
    m.fetchFlowHistory.mockImplementation(() => ok(history({ total: 1_234_567 })));
    renderPage();
    expect(await screen.findByText(/of 1,234,567/)).toBeInTheDocument();
  });
});

describe('FlowHistory: races', () => {
  it('ignores a slow earlier response that arrives after a newer search', async () => {
    let releaseSlow: (v: unknown) => void = () => {};
    m.fetchFlowTimeline.mockImplementationOnce(() => ok(timeline({ total: 111 })));
    m.fetchFlowHistory.mockImplementationOnce(() => ok(history({ total: 111 })));
    renderPage();
    await screen.findByText('111');

    // Search 2 is slow; search 3 is fast and finishes first.
    m.fetchFlowTimeline.mockImplementationOnce(() => new Promise((r) => { releaseSlow = r; }) as never);
    m.fetchFlowHistory.mockImplementationOnce(() => ok(history({ total: 222 })));
    search();
    m.fetchFlowTimeline.mockImplementationOnce(() => ok(timeline({ total: 333 })));
    m.fetchFlowHistory.mockImplementationOnce(() => ok(history({ total: 333 })));
    fireEvent.click(screen.getByRole('button', { name: /^Search|Searching/ }));
    await screen.findByText('333');

    await act(async () => { releaseSlow({ data: timeline({ total: 222 }) }); });
    expect(screen.getByText('333')).toBeInTheDocument();
    expect(screen.queryByText('222')).not.toBeInTheDocument();
  });
});

describe('FlowHistory: how complete is it?', () => {
  it('always offers the capture note', async () => {
    renderPage();
    const details = (await screen.findByText('How complete is this?')).closest('details')!;
    expect(within(details).getByText(/counts are lower bounds/)).toBeInTheDocument();
  });

  it('warns that history is in memory when it is not durable', async () => {
    m.fetchFlowTimeline.mockImplementation(() => ok(timeline({ coverage: { ...COVERAGE, durable: false } })));
    m.fetchFlowHistory.mockImplementation(() => ok(history({ coverage: { ...COVERAGE, durable: false } })));
    renderPage();
    expect(await screen.findByText(/held in memory/)).toBeInTheDocument();
    expect(screen.getByText(/PAQTRA_DATA_DIR/)).toBeInTheDocument();
  });

  it('warns when the last capture failed', async () => {
    const failing = { ...CAPTURE, last_capture_ok: false };
    m.fetchFlowTimeline.mockImplementation(() => ok(timeline({ capture: failing })));
    m.fetchFlowHistory.mockImplementation(() => ok(history({ capture: failing })));
    renderPage();
    expect(await screen.findByText(/last flow capture did not succeed/)).toBeInTheDocument();
  });

  it('says where history begins when the range reaches before it, without claiming data was lost', async () => {
    const early = { ...COVERAGE, range_starts_before_oldest: true };
    m.fetchFlowTimeline.mockImplementation(() => ok(timeline({ coverage: early })));
    m.fetchFlowHistory.mockImplementation(() => ok(history({ coverage: early })));
    renderPage();
    const note = await screen.findByText(/Stored history begins/);
    expect(note.textContent).toMatch(/nothing from before then was captured, or it has aged out/);
    expect(note.textContent).toMatch(/kept 7 days/);
  });

  it('shows no coverage warnings when everything is healthy and the range is covered', async () => {
    renderPage();
    await screen.findByText('flows found');
    expect(screen.queryByText(/held in memory/)).not.toBeInTheDocument();
    expect(screen.queryByText(/did not succeed/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Stored history begins/)).not.toBeInTheDocument();
  });
});
