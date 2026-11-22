// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { render, screen, waitFor, fireEvent, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { AxiosError, AxiosResponse } from 'axios';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import * as api from '../../../services/api';
import * as files from '../../../services/reportFiles';
import { useAuthStore } from '../../../stores/authStore';
import Compliance from '../index';

vi.mock('../../../services/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../services/api')>();
  return { ...actual, fetchFrameworks: vi.fn(), fetchAudits: vi.fn(), runAudit: vi.fn(), fetchAuditReport: vi.fn() };
});
vi.mock('../../../services/reportFiles', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../services/reportFiles')>();
  return { ...actual, openBlobInNewTab: vi.fn(), downloadBlob: vi.fn() };
});

const m = vi.mocked(api);
const f = vi.mocked(files);
const ok = <T,>(data: T) => Promise.resolve({ data } as never);
const httpError = (status: number, error: string) =>
  Promise.reject(new AxiosError('x', String(status), undefined, undefined, { status, data: { error } } as AxiosResponse));

const FRAMEWORKS = [
  { id: 'pci-dss-4.0', name: 'PCI-DSS', version: '4.0', description: 'd', control_count: 64 },
  { id: 'hipaa', name: 'HIPAA', version: '2013', description: 'd', control_count: 42 },
];
const SUMMARY = { audit_id: '11111111-aaaa-bbbb-cccc-222222222222', framework: 'hipaa', completed_at: '2026-09-24T04:00:00+00:00', requested_by: 'eda', total_controls: 4, passed: 2, failed: 1, skipped: 1, score: 66.7 };
const RESULT = {
  ...SUMMARY, status: 'completed', started_at: '2026-09-24T03:59:59+00:00', cluster: null, flows_sampled: 10,
  findings: [
    { control_id: 'HIPAA-NET-1', title: 'Default-deny network policies', status: 'skipped', severity: 'high', description: 'Namespaces could not be listed' },
    { control_id: 'HIPAA-MON-1', title: 'Network flow monitoring', status: 'passed', severity: 'high', description: 'Hubble reachable' },
    { control_id: 'HIPAA-ENC-1', title: 'Encryption in transit', status: 'failed', severity: 'critical', description: 'No encryption' },
    { control_id: 'HIPAA-DRP-1', title: 'Dropped network flows', status: 'passed', severity: 'medium', description: 'none' },
  ],
};

const renderPage = () => render(<MemoryRouter><Compliance /></MemoryRouter>);

beforeEach(() => {
  vi.resetAllMocks();
  useAuthStore.setState({ role: 'editor' });
  m.fetchFrameworks.mockImplementation(() => ok({ frameworks: FRAMEWORKS, total: 2 }));
  m.fetchAudits.mockImplementation(() => ok({ audits: [], total: 0 }));
});

describe('Compliance', () => {
  it('says what the checks are, and that they are not a compliance assessment', async () => {
    renderPage();
    expect(await screen.findByText('They are not a compliance assessment.')).toBeInTheDocument();
    expect(screen.getByText(/never as passed/)).toBeInTheDocument();
  });

  it('files a run under the chosen framework and explains what that means', async () => {
    renderPage();
    const select = await screen.findByLabelText('File under');
    expect(within(select).getAllByRole('option').map((o) => o.textContent)).toEqual(['PCI-DSS 4.0', 'HIPAA 2013']);
    expect(screen.getByText(/PCI-DSS 4.0 defines 64 controls; the checks below are not mapped to them/)).toBeInTheDocument();
    fireEvent.change(select, { target: { value: 'hipaa' } });
    expect(screen.getByText(/HIPAA 2013 defines 42 controls/)).toBeInTheDocument();
  });

  it('runs the checks, shows every result with skipped as "not evaluated", and refreshes the history', async () => {
    m.runAudit.mockImplementation(() => ok(RESULT));
    renderPage();
    fireEvent.change(await screen.findByLabelText('File under'), { target: { value: 'hipaa' } });
    m.fetchAudits.mockImplementation(() => ok({ audits: [SUMMARY], total: 1 }));
    fireEvent.click(screen.getByRole('button', { name: 'Run network checks' }));
    await waitFor(() => expect(m.runAudit).toHaveBeenCalledWith('hipaa'));
    expect(await screen.findByText('2 of 4 checks passed')).toBeInTheDocument();
    expect(screen.getByText(/1 failed, 1 not evaluated; 66.7% of the evaluated checks passed/)).toBeInTheDocument();
    expect(screen.getByText('NOT EVALUATED')).toBeInTheDocument();
    expect(screen.getAllByText('PASSED')).toHaveLength(2);
    expect(screen.getByText('FAILED')).toBeInTheDocument();
    expect(screen.getByText(/10 most recent flows, not a period of time/)).toBeInTheDocument();
    expect(await screen.findByText('2 of 4 passed')).toBeInTheDocument();
  });

  it('shows the server reason when a run is refused', async () => {
    m.runAudit.mockImplementation(() => httpError(403, 'Editor or admin role required'));
    renderPage();
    await screen.findByLabelText('File under');
    fireEvent.click(screen.getByRole('button', { name: 'Run network checks' }));
    expect(await screen.findByText('Editor or admin role required')).toBeInTheDocument();
    expect(screen.queryByText(/checks passed/)).not.toBeInTheDocument();
  });

  it('hides the run control from viewers but still shows stored runs', async () => {
    useAuthStore.setState({ role: 'viewer' });
    m.fetchAudits.mockImplementation(() => ok({ audits: [SUMMARY], total: 1 }));
    renderPage();
    expect(await screen.findByText('2 of 4 passed')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Run network checks' })).not.toBeInTheDocument();
  });

  it('says so when nothing is stored yet', async () => {
    renderPage();
    expect(await screen.findByText(/No runs stored yet\. Run the checks above/)).toBeInTheDocument();
  });

  it('lists a stored run with counts, requester and framework name', async () => {
    m.fetchAudits.mockImplementation(() => ok({ audits: [SUMMARY], total: 1 }));
    renderPage();
    expect(await screen.findByText('HIPAA 2013', { selector: 'b' })).toBeInTheDocument();
    expect(screen.getByText(/1 failed · 1 not evaluated/)).toBeInTheDocument();
    expect(screen.getByText(/by eda/)).toBeInTheDocument();
  });

  it('opens the HTML report in a new tab from the fetched blob', async () => {
    m.fetchAudits.mockImplementation(() => ok({ audits: [SUMMARY], total: 1 }));
    const blob = new Blob(['<html></html>']);
    m.fetchAuditReport.mockImplementation(() => ok(blob));
    renderPage();
    fireEvent.click(await screen.findByRole('button', { name: 'Open report for 11111111' }));
    await waitFor(() => expect(m.fetchAuditReport).toHaveBeenCalledWith(SUMMARY.audit_id, 'html'));
    await waitFor(() => expect(f.openBlobInNewTab).toHaveBeenCalledWith(blob));
    expect(f.downloadBlob).not.toHaveBeenCalled();
  });

  it('downloads CSV and JSON with a safe file name', async () => {
    m.fetchAudits.mockImplementation(() => ok({ audits: [SUMMARY], total: 1 }));
    const blob = new Blob(['x']);
    m.fetchAuditReport.mockImplementation(() => ok(blob));
    renderPage();
    fireEvent.click(await screen.findByRole('button', { name: 'Download CSV for 11111111' }));
    await waitFor(() => expect(f.downloadBlob).toHaveBeenCalledWith(blob, 'paqtra-checks-hipaa-11111111.csv'));
    fireEvent.click(screen.getByRole('button', { name: 'Download JSON for 11111111' }));
    await waitFor(() => expect(f.downloadBlob).toHaveBeenLastCalledWith(blob, 'paqtra-checks-hipaa-11111111.json'));
    expect(m.fetchAuditReport).toHaveBeenCalledWith(SUMMARY.audit_id, 'csv');
    expect(f.openBlobInNewTab).not.toHaveBeenCalled();
  });

  it('reads the reason out of a failed report request (a Blob body)', async () => {
    m.fetchAudits.mockImplementation(() => ok({ audits: [SUMMARY], total: 1 }));
    m.fetchAuditReport.mockImplementation(() =>
      Promise.reject(new AxiosError('Request failed with status code 404', '404', undefined, undefined, { status: 404, data: new Blob(['{"error":"Audit not found"}']) } as AxiosResponse)),
    );
    renderPage();
    fireEvent.click(await screen.findByRole('button', { name: 'Open report for 11111111' }));
    expect(await screen.findByText('Audit not found')).toBeInTheDocument();
    expect(f.openBlobInNewTab).not.toHaveBeenCalled();
  });

  it('shows a load failure', async () => {
    m.fetchAudits.mockImplementation(() => httpError(500, 'boom'));
    renderPage();
    expect(await screen.findByText('boom')).toBeInTheDocument();
  });
});
