// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { AxiosError, AxiosResponse } from 'axios';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import * as api from '../../../services/api';
import PolicyRules from '../index';

vi.mock('../../../services/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../services/api')>();
  return {
    ...actual,
    fetchPolicies: vi.fn(),
    fetchPolicy: vi.fn(),
    addPolicyRule: vi.fn(),
    updatePolicyRule: vi.fn(),
    deletePolicyRule: vi.fn(),
    simulatePolicy: vi.fn(),
  };
});

const m = vi.mocked(api);
const ok = <T,>(data: T) => Promise.resolve({ data } as never);
const apiError = (status: number, error: string) =>
  Promise.reject(new AxiosError('x', String(status), undefined, undefined, { status, data: { error } } as AxiosResponse));

const DETAIL = {
  id: 'uid-1', name: 'db', namespace: 'team-a', created_at: '', status: 'active', resource_version: '42',
  rule_counts: { ingress: 2, egress: 0, ingressDeny: 0, egressDeny: 0 },
  spec: {
    endpointSelector: { matchLabels: { app: 'db' } },
    ingress: [{ fromCIDR: ['10.0.0.0/8'] }, { fromEntities: ['world'] }],
  },
};

function renderPage(url = '/policy-rules') {
  return render(<MemoryRouter initialEntries={[url]}><PolicyRules /></MemoryRouter>);
}
const choosePolicy = async () => {
  await screen.findByRole('option', { name: 'team-a/db' });
  fireEvent.change(screen.getByLabelText('Policy'), { target: { value: 'uid-1' } });
  await screen.findByText(/Ingress \(allow\)/);
};

beforeEach(() => {
  vi.resetAllMocks();
  m.fetchPolicies.mockImplementation(() => ok({ policies: [{ id: 'uid-1', name: 'db', namespace: 'team-a', created_at: '', status: 'active' }] }));
  m.fetchPolicy.mockImplementation(() => ok(DETAIL));
  vi.spyOn(window, 'confirm').mockReturnValue(true);
});

describe('PolicyRules', () => {
  it('lists a policy\'s rules per direction', async () => {
    renderPage();
    await choosePolicy();
    expect(screen.getByText(/10\.0\.0\.0\/8/)).toBeInTheDocument();
    expect(screen.getByText(/"world"/)).toBeInTheDocument();
    expect(screen.getByText('No egress rules')).toBeInTheDocument();
  });

  it('preselects the policy from ?policy=', async () => {
    renderPage('/policy-rules?policy=uid-1');
    expect(await screen.findByText(/Ingress \(allow\)/)).toBeInTheDocument();
    expect(m.fetchPolicy).toHaveBeenCalledWith('uid-1');
  });

  it('adds a rule, sending the resource_version it read', async () => {
    m.addPolicyRule.mockImplementation(() => ok({}));
    renderPage();
    await choosePolicy();
    fireEvent.click(screen.getByRole('button', { name: /Add egress rule/ }));
    fireEvent.change(screen.getByLabelText('Rule JSON'), { target: { value: '{"toEntities":["dns"]}' } });
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    await waitFor(() => expect(m.addPolicyRule).toHaveBeenCalledWith('uid-1', { direction: 'egress', rule: { toEntities: ['dns'] }, resource_version: '42' }, false));
    expect(await screen.findByText('Rule added')).toBeInTheDocument();
    expect(m.fetchPolicy.mock.calls.length).toBeGreaterThan(1);
  });

  it('offers templates for the newer rule kinds and applies the chosen one', async () => {
    m.addPolicyRule.mockImplementation(() => ok({}));
    renderPage();
    await choosePolicy();
    fireEvent.click(screen.getByRole('button', { name: /Add egress rule/ }));
    fireEvent.change(screen.getByLabelText('Template'), { target: { value: '1' } });
    expect(screen.getByLabelText('Rule JSON')).toHaveDisplayValue(/toFQDNs/);
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    await waitFor(() => expect(m.addPolicyRule).toHaveBeenCalled());
    expect(m.addPolicyRule.mock.calls[0][1].rule).toHaveProperty('toFQDNs');
  });

  it('has no template picker when editing an existing rule', async () => {
    renderPage();
    await choosePolicy();
    fireEvent.click(screen.getByRole('button', { name: 'Edit ingress rule 1' }));
    expect(screen.queryByLabelText('Template')).not.toBeInTheDocument();
  });

  it('rejects invalid JSON without calling the API', async () => {
    renderPage();
    await choosePolicy();
    fireEvent.click(screen.getByRole('button', { name: /Add ingress rule/ }));
    fireEvent.change(screen.getByLabelText('Rule JSON'), { target: { value: '{nope' } });
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    expect(await screen.findByText(/Invalid rule JSON/)).toBeInTheDocument();
    expect(m.addPolicyRule).not.toHaveBeenCalled();
  });

  it('edits rule #2 in place', async () => {
    m.updatePolicyRule.mockImplementation(() => ok({}));
    renderPage();
    await choosePolicy();
    fireEvent.click(screen.getByRole('button', { name: 'Edit ingress rule 2' }));
    fireEvent.change(screen.getByLabelText('Rule JSON'), { target: { value: '{"fromEntities":["cluster"]}' } });
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    await waitFor(() => expect(m.updatePolicyRule).toHaveBeenCalledWith('uid-1', { direction: 'ingress', index: 1, rule: { fromEntities: ['cluster'] }, resource_version: '42' }, false));
    expect(await screen.findByText('Rule updated')).toBeInTheDocument();
  });

  it('previews with a dry run, then simulates the resulting spec', async () => {
    const change = { policy: 'team-a/db', action: 'add', direction: 'egress', dry_run: true, rule_counts: { ingress: 2, egress: 1, ingressDeny: 0, egressDeny: 0 }, spec: { egress: [{}] }, result: { index: 0 } };
    m.addPolicyRule.mockImplementation(() => ok(change));
    m.simulatePolicy.mockImplementation(() => ok({ unknown: ['toFQDNs'] }));
    renderPage();
    await choosePolicy();
    fireEvent.click(screen.getByRole('button', { name: /Add egress rule/ }));
    fireEvent.click(screen.getByRole('button', { name: /Preview/ }));
    expect(await screen.findByText(/Dry run passed/)).toBeInTheDocument();
    expect(m.addPolicyRule.mock.calls[0][2]).toBe(true);
    expect(m.simulatePolicy).toHaveBeenCalledWith({ name: 'db', namespace: 'team-a', spec: change.spec });
    expect(screen.getByText(/toFQDNs/)).toBeInTheDocument();
  });

  it('deletes a rule only after confirmation', async () => {
    m.deletePolicyRule.mockImplementation(() => ok({}));
    renderPage();
    await choosePolicy();
    vi.spyOn(window, 'confirm').mockReturnValueOnce(false);
    fireEvent.click(screen.getByRole('button', { name: 'Delete ingress rule 1' }));
    expect(m.deletePolicyRule).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Delete ingress rule 1' }));
    await waitFor(() => expect(m.deletePolicyRule).toHaveBeenCalledWith('uid-1', { direction: 'ingress', index: 0, resource_version: '42' }));
    expect(await screen.findByText('Rule deleted')).toBeInTheDocument();
  });

  it('shows why the last rule cannot be deleted', async () => {
    m.deletePolicyRule.mockImplementation(() => apiError(400, "refusing to delete the policy's last rule: Cilium rejects a policy with no rules; delete the policy instead"));
    renderPage();
    await choosePolicy();
    fireEvent.click(screen.getByRole('button', { name: 'Delete ingress rule 1' }));
    expect(await screen.findByText(/delete the policy instead/)).toBeInTheDocument();
    expect(m.deletePolicyRule).toHaveBeenCalledTimes(1);
  });

  it('reloads the policy when the server reports a conflict', async () => {
    m.addPolicyRule.mockImplementation(() => apiError(409, 'policy changed since it was read'));
    renderPage();
    await choosePolicy();
    const before = m.fetchPolicy.mock.calls.length;
    fireEvent.click(screen.getByRole('button', { name: /Add ingress rule/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
    expect(await screen.findByText('policy changed since it was read')).toBeInTheDocument();
    await waitFor(() => expect(m.fetchPolicy.mock.calls.length).toBeGreaterThan(before));
  });
});
