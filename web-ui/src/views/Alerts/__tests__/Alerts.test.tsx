// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { render, screen, waitFor, fireEvent, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { AxiosError, AxiosResponse } from 'axios';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import * as api from '../../../services/api';
import Alerts from '../index';

vi.mock('../../../services/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../services/api')>();
  return {
    ...actual,
    fetchAlertRules: vi.fn(),
    fetchAlertHistory: vi.fn(),
    toggleAlertRule: vi.fn(),
    createAlertRule: vi.fn(),
    updateAlertRule: vi.fn(),
    deleteAlertRule: vi.fn(),
    fetchChannels: vi.fn(),
    createChannel: vi.fn(),
    deleteChannel: vi.fn(),
    testChannel: vi.fn(),
    fetchSilences: vi.fn(),
    createSilence: vi.fn(),
    deleteSilence: vi.fn(),
  };
});

const m = vi.mocked(api);
const ok = <T,>(data: T) => Promise.resolve({ data } as never);
const forbidden = () => Promise.reject(new AxiosError('Forbidden', '403', undefined, undefined, { status: 403, data: { error: 'Admin role required' } } as AxiosResponse));

// Exactly what GET /alerts/rules returns for the seeded defaults: no channels, no trigger_count.
const SEEDED_RULES = [
  { id: 'rule-001', name: 'High Drop Rate', condition: 'drop_rate > 5% for 5m', severity: 'critical', enabled: true },
  { id: 'rule-004', name: 'Endpoint Unhealthy', condition: 'endpoint_status != ready', severity: 'high', enabled: false },
];
const future = (mins: number) => new Date(Date.now() + mins * 60_000).toISOString();

function renderPage() {
  return render(<MemoryRouter><Alerts /></MemoryRouter>);
}
const openTab = (label: RegExp) => fireEvent.click(screen.getByRole('button', { name: label }));

beforeEach(() => {
  vi.resetAllMocks();
  m.fetchAlertRules.mockImplementation(() => ok({ rules: SEEDED_RULES }));
  m.fetchAlertHistory.mockImplementation(() => ok({ alerts: [] }));
  m.fetchChannels.mockImplementation(() => ok({ channels: [] }));
  m.fetchSilences.mockImplementation(() => ok({ silences: [] }));
  vi.spyOn(window, 'confirm').mockReturnValue(true);
});

describe('Alerts: rules', () => {
  it('renders seeded rules that have no channels or trigger_count (used to crash on channels.join)', async () => {
    renderPage();
    expect(await screen.findByText('High Drop Rate')).toBeInTheDocument();
    expect(screen.getByText('Endpoint Unhealthy')).toBeInTheDocument();
    expect(screen.getAllByText('Triggered 0x')).toHaveLength(2);
  });

  it('toggles a rule and refreshes', async () => {
    m.toggleAlertRule.mockImplementation(() => ok({}));
    renderPage();
    await screen.findByText('High Drop Rate');
    fireEvent.click(screen.getByRole('button', { name: 'Disable' }));
    await waitFor(() => expect(m.toggleAlertRule).toHaveBeenCalledWith('rule-001'));
    expect(await screen.findByText('High Drop Rate disabled')).toBeInTheDocument();
    await waitFor(() => expect(m.fetchAlertRules.mock.calls.length).toBeGreaterThan(1));
  });

  it('shows the server error when a toggle is refused', async () => {
    m.toggleAlertRule.mockImplementation(() => Promise.reject(new AxiosError('x', '403', undefined, undefined, { status: 403, data: { error: 'Admin role required' } } as AxiosResponse)));
    renderPage();
    await screen.findByText('High Drop Rate');
    fireEvent.click(screen.getByRole('button', { name: 'Disable' }));
    expect(await screen.findByText('Admin role required')).toBeInTheDocument();
  });

  it('marks rules that an active silence covers, and ignores expired silences', async () => {
    m.fetchSilences.mockImplementation(() => ok({ silences: [
      { id: 's1', rule_id: 'rule-001', comment: '', created_by: 'admin', created_at: '', until: future(30) },
      { id: 's2', rule_id: 'rule-004', comment: '', created_by: 'admin', created_at: '', until: new Date(Date.now() - 60_000).toISOString() },
    ] }));
    renderPage();
    await screen.findByText('High Drop Rate');
    await waitFor(() => expect(screen.getAllByText(/silenced until/)).toHaveLength(1));
    expect(screen.getByRole('button', { name: /Silences \(1\)/ })).toBeInTheDocument();
  });

  it('an all-rules silence marks every rule', async () => {
    m.fetchSilences.mockImplementation(() => ok({ silences: [{ id: 's1', rule_id: null, comment: '', created_by: 'admin', created_at: '', until: future(30) }] }));
    renderPage();
    await screen.findByText('High Drop Rate');
    await waitFor(() => expect(screen.getAllByText(/silenced until/)).toHaveLength(2));
  });
});

describe('Alerts: rule add / edit / delete', () => {
  const apiError = (status: number, error: string) =>
    Promise.reject(new AxiosError('x', String(status), undefined, undefined, { status, data: { error } } as AxiosResponse));

  it('creates a rule and refreshes', async () => {
    m.createAlertRule.mockImplementation(() => ok({}));
    renderPage();
    await screen.findByText('High Drop Rate');
    fireEvent.click(screen.getByRole('button', { name: /New rule/ }));
    fireEvent.change(screen.getByPlaceholderText('High drop rate'), { target: { value: ' Slow DNS ' } });
    fireEvent.change(screen.getByPlaceholderText('drop_rate > 5% for 5m'), { target: { value: 'dns_servfail > 10/min' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    await waitFor(() => expect(m.createAlertRule).toHaveBeenCalledWith({ name: 'Slow DNS', condition: 'dns_servfail > 10/min', severity: 'warning' }));
    expect(await screen.findByText('Slow DNS created')).toBeInTheDocument();
    await waitFor(() => expect(m.fetchAlertRules.mock.calls.length).toBeGreaterThan(1));
  });

  it('shows the server validation error and keeps the form open', async () => {
    m.createAlertRule.mockImplementation(() => apiError(400, 'unsupported condition'));
    renderPage();
    await screen.findByText('High Drop Rate');
    fireEvent.click(screen.getByRole('button', { name: /New rule/ }));
    fireEvent.change(screen.getByPlaceholderText('High drop rate'), { target: { value: 'x' } });
    fireEvent.change(screen.getByPlaceholderText('drop_rate > 5% for 5m'), { target: { value: 'cpu > 90%' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    expect(await screen.findByText('unsupported condition')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Create' })).toBeEnabled();
  });

  it('edits a rule in place', async () => {
    m.updateAlertRule.mockImplementation(() => ok({}));
    renderPage();
    await screen.findByText('High Drop Rate');
    fireEvent.click(screen.getByRole('button', { name: 'Edit High Drop Rate' }));
    fireEvent.change(screen.getByDisplayValue('drop_rate > 5% for 5m'), { target: { value: 'drop_rate > 8% for 5m' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save' }));
    await waitFor(() => expect(m.updateAlertRule).toHaveBeenCalledWith('rule-001', { name: 'High Drop Rate', condition: 'drop_rate > 8% for 5m', severity: 'critical' }));
    expect(await screen.findByText('High Drop Rate updated')).toBeInTheDocument();
  });

  it('deletes a rule after confirmation, and not when declined', async () => {
    m.deleteAlertRule.mockImplementation(() => ok({}));
    renderPage();
    await screen.findByText('High Drop Rate');
    vi.spyOn(window, 'confirm').mockReturnValueOnce(false);
    fireEvent.click(screen.getByRole('button', { name: 'Delete High Drop Rate' }));
    expect(m.deleteAlertRule).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Delete High Drop Rate' }));
    await waitFor(() => expect(m.deleteAlertRule).toHaveBeenCalledWith('rule-001'));
    expect(await screen.findByText('High Drop Rate deleted')).toBeInTheDocument();
  });

  it('asks again before force-deleting a built-in rule', async () => {
    m.deleteAlertRule
      .mockImplementationOnce(() => apiError(400, 'built-in rule: disable it instead, or pass force=true to delete'))
      .mockImplementationOnce(() => ok({}));
    renderPage();
    await screen.findByText('High Drop Rate');
    fireEvent.click(screen.getByRole('button', { name: 'Delete High Drop Rate' }));
    await waitFor(() => expect(m.deleteAlertRule).toHaveBeenLastCalledWith('rule-001', true));
    expect(await screen.findByText('High Drop Rate deleted')).toBeInTheDocument();
  });
});

describe('Alerts: history', () => {
  it('flags alerts whose notification was silenced', async () => {
    m.fetchAlertHistory.mockImplementation(() => ok({ alerts: [
      { id: 'a1', rule_id: 'rule-001', rule_name: 'High Drop Rate', timestamp: '2026-01-01T00:00:00Z', fired_at: '2026-01-01T00:00:00Z', status: 'firing', severity: 'critical', message: 'drops', namespace: '', pod: '', silenced: true },
    ] }));
    renderPage();
    await screen.findByText('High Drop Rate');
    openTab(/History/);
    expect(await screen.findByText('silenced')).toBeInTheDocument();
  });
});

describe('Alerts: channels', () => {
  const channel = { id: 'ch-1', name: 'on-call', kind: 'slack', target: 'https://hooks.slack.com/…', min_severity: 'high', enabled: true, created_at: '' };

  it('lists channels with the masked destination', async () => {
    m.fetchChannels.mockImplementation(() => ok({ channels: [channel] }));
    renderPage();
    await screen.findByText('High Drop Rate');
    openTab(/Channels/);
    expect(await screen.findByText('on-call')).toBeInTheDocument();
    expect(screen.getByText('https://hooks.slack.com/…')).toBeInTheDocument();
    expect(screen.getByText('≥ high')).toBeInTheDocument();
  });

  it('explains what happens with no channels', async () => {
    renderPage();
    await screen.findByText('High Drop Rate');
    openTab(/Channels/);
    expect(await screen.findByText(/not sent anywhere/)).toBeInTheDocument();
  });

  it('adds a PagerDuty channel with a minimum severity', async () => {
    m.createChannel.mockImplementation(() => ok({}));
    renderPage();
    await screen.findByText('High Drop Rate');
    openTab(/Channels/);
    fireEvent.click(await screen.findByRole('button', { name: /Add channel/ }));
    fireEvent.change(screen.getByPlaceholderText('on-call'), { target: { value: '  pager  ' } });
    fireEvent.change(screen.getByDisplayValue('Slack'), { target: { value: 'pagerduty' } });
    fireEvent.change(screen.getByPlaceholderText('routing key'), { target: { value: ' key123 ' } });
    fireEvent.change(screen.getByDisplayValue('Any'), { target: { value: 'critical' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    await waitFor(() => expect(m.createChannel).toHaveBeenCalledWith({ name: 'pager', kind: 'pagerduty', target: 'key123', min_severity: 'critical' }));
    expect(await screen.findByText('Channel added')).toBeInTheDocument();
  });

  it('keeps the form and shows the reason when creating fails', async () => {
    m.createChannel.mockImplementation(() => Promise.reject(new AxiosError('x', '400', undefined, undefined, { status: 400, data: { error: 'target must be an http(s) URL' } } as AxiosResponse)));
    renderPage();
    await screen.findByText('High Drop Rate');
    openTab(/Channels/);
    fireEvent.click(await screen.findByRole('button', { name: /Add channel/ }));
    fireEvent.change(screen.getByPlaceholderText('on-call'), { target: { value: 'x' } });
    fireEvent.change(screen.getByPlaceholderText('https://…'), { target: { value: 'ftp://nope' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    expect(await screen.findByText('target must be an http(s) URL')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('on-call')).toHaveValue('x');
  });

  it('reports a delivered and a failed test notification', async () => {
    m.fetchChannels.mockImplementation(() => ok({ channels: [channel] }));
    renderPage();
    await screen.findByText('High Drop Rate');
    openTab(/Channels/);
    await screen.findByText('on-call');

    m.testChannel.mockImplementationOnce(() => ok({ channel_id: 'ch-1', channel_name: 'on-call', delivered: true, attempts: 1, error: null }));
    fireEvent.click(screen.getByRole('button', { name: /Test/ }));
    expect(await screen.findByText('Test notification delivered to on-call')).toBeInTheDocument();

    m.testChannel.mockImplementationOnce(() => ok({ channel_id: 'ch-1', channel_name: 'on-call', delivered: false, attempts: 3, error: 'HTTP 500 Internal Server Error' }));
    fireEvent.click(screen.getByRole('button', { name: /Test/ }));
    expect(await screen.findByText(/failed after 3 attempt\(s\): HTTP 500/)).toBeInTheDocument();
  });

  it('deletes only after confirmation', async () => {
    m.fetchChannels.mockImplementation(() => ok({ channels: [channel] }));
    m.deleteChannel.mockImplementation(() => ok({}));
    renderPage();
    await screen.findByText('High Drop Rate');
    openTab(/Channels/);
    await screen.findByText('on-call');

    vi.mocked(window.confirm).mockReturnValueOnce(false);
    fireEvent.click(screen.getByTitle('Delete channel'));
    expect(m.deleteChannel).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTitle('Delete channel'));
    await waitFor(() => expect(m.deleteChannel).toHaveBeenCalledWith('ch-1'));
  });
});

describe('Alerts: silences', () => {
  it('creates a silence for one rule with a duration and reason', async () => {
    m.createSilence.mockImplementation(() => ok({}));
    renderPage();
    await screen.findByText('High Drop Rate');
    openTab(/Silences/);
    fireEvent.click(await screen.findByRole('button', { name: /New silence/ }));
    fireEvent.change(screen.getByDisplayValue('All rules'), { target: { value: 'rule-001' } });
    fireEvent.change(screen.getByDisplayValue('1 hour'), { target: { value: '240' } });
    fireEvent.change(screen.getByPlaceholderText('planned maintenance'), { target: { value: ' upgrade ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Silence' }));
    await waitFor(() => expect(m.createSilence).toHaveBeenCalledWith({ rule_id: 'rule-001', duration_minutes: 240, comment: 'upgrade' }));
  });

  it('omits rule_id when silencing all rules', async () => {
    m.createSilence.mockImplementation(() => ok({}));
    renderPage();
    await screen.findByText('High Drop Rate');
    openTab(/Silences/);
    fireEvent.click(await screen.findByRole('button', { name: /New silence/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Silence' }));
    await waitFor(() => expect(m.createSilence).toHaveBeenCalledWith({ rule_id: undefined, duration_minutes: 60, comment: undefined }));
  });

  it('lists active silences with rule name and ends one', async () => {
    m.fetchSilences.mockImplementation(() => ok({ silences: [{ id: 's1', rule_id: 'rule-001', comment: 'maint', created_by: 'admin', created_at: '', until: future(30) }] }));
    m.deleteSilence.mockImplementation(() => ok({}));
    renderPage();
    await screen.findByText('High Drop Rate');
    openTab(/Silences/);
    const row = (await screen.findByText(/by admin · maint/)).closest('div.flex.items-center') as HTMLElement;
    expect(within(row).getByText('High Drop Rate')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'End silence' }));
    await waitFor(() => expect(m.deleteSilence).toHaveBeenCalledWith('s1'));
  });
});

describe('Alerts: non-admin', () => {
  it('still shows rules and history when channels and silences are forbidden', async () => {
    m.fetchChannels.mockImplementation(forbidden);
    m.fetchSilences.mockImplementation(forbidden);
    renderPage();
    expect(await screen.findByText('High Drop Rate')).toBeInTheDocument();
    openTab(/Channels/);
    expect(await screen.findByText(/Admin role required/)).toBeInTheDocument();
    openTab(/Silences/);
    expect(await screen.findByText(/Admin role required/)).toBeInTheDocument();
  });
});
