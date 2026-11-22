// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { AxiosError, AxiosResponse } from 'axios';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import * as api from '../../../services/api';
import { useAuthStore } from '../../../stores/authStore';
import { Board } from '../../../components/Board';
import ChangePassword from '../ChangePassword';

vi.mock('../../../services/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../../services/api')>();
  return { ...actual, fetchMe: vi.fn(), changePassword: vi.fn() };
});

const m = vi.mocked(api);
const ok = <T,>(data: T) => Promise.resolve({ data } as never);
const renderCard = () => render(<Board><ChangePassword /></Board>);
const fields = () => Array.from(document.querySelectorAll('input[type="password"]')) as HTMLInputElement[];
const fill = (cur: string, next: string, confirm: string) => {
  const [c, n, f] = fields();
  fireEvent.change(c, { target: { value: cur } });
  fireEvent.change(n, { target: { value: next } });
  fireEvent.change(f, { target: { value: confirm } });
};

beforeEach(() => {
  vi.resetAllMocks();
  m.fetchMe.mockImplementation(() => ok({ username: 'bob', role: 'viewer', source: 'local' }));
});

describe('ChangePassword', () => {
  it('explains that the configured admin password is an environment setting', async () => {
    m.fetchMe.mockImplementation(() => ok({ username: 'admin', role: 'admin', source: 'config' }));
    renderCard();
    expect(await screen.findByText(/ADMIN_PASSWORD/)).toBeInTheDocument();
    expect(fields()).toHaveLength(0);
  });

  it('will not submit until the new password is confirmed', async () => {
    renderCard();
    await waitFor(() => expect(fields()).toHaveLength(3));
    const submit = screen.getByRole('button', { name: 'Change password' });
    fill('old-password-12', 'new-password-12', 'new-password-XX');
    expect(submit).toBeDisabled();
    expect(screen.getByText('Passwords do not match.')).toBeInTheDocument();
    fill('old-password-12', 'new-password-12', 'new-password-12');
    expect(submit).toBeEnabled();
  });

  it('changes the password, then signs out because every old token is revoked', async () => {
    m.changePassword.mockImplementation(() => ok({ changed: true, reauthenticate: true }));
    const logout = vi.fn();
    useAuthStore.setState({ logout });
    renderCard();
    await waitFor(() => expect(fields()).toHaveLength(3));
    fill('old-password-12', 'new-password-12', 'new-password-12');
    fireEvent.click(screen.getByRole('button', { name: 'Change password' }));
    await waitFor(() => expect(m.changePassword).toHaveBeenCalledWith({ current_password: 'old-password-12', new_password: 'new-password-12' }));
    expect(await screen.findByText(/Signing you out/)).toBeInTheDocument();
    await waitFor(() => expect(logout).toHaveBeenCalled(), { timeout: 3000 });
  });

  it('shows why a change was refused and does not sign out', async () => {
    m.changePassword.mockImplementation(() => Promise.reject(new AxiosError('x', '400', undefined, undefined, { status: 400, data: { error: 'Current password is incorrect' } } as AxiosResponse)));
    const logout = vi.fn();
    useAuthStore.setState({ logout });
    renderCard();
    await waitFor(() => expect(fields()).toHaveLength(3));
    fill('wrong-password-1', 'new-password-12', 'new-password-12');
    fireEvent.click(screen.getByRole('button', { name: 'Change password' }));
    expect(await screen.findByText('Current password is incorrect')).toBeInTheDocument();
    expect(logout).not.toHaveBeenCalled();
    expect(fields()).toHaveLength(3);
  });
});
