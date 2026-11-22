// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { AxiosError, InternalAxiosRequestConfig } from 'axios';
import api, { UNAUTHORIZED_EVENT } from '../services/api';
import { useAuthStore } from '../stores/authStore';

const json = (status: number, body: unknown) =>
  Promise.resolve({ status, ok: status >= 200 && status < 300, json: () => Promise.resolve(body) } as Response);

function reset() {
  localStorage.clear();
  useAuthStore.setState({ token: null, username: null, role: null, isAuthenticated: false, sessionReady: false, authRequired: true });
}

beforeEach(reset);
afterEach(() => vi.unstubAllGlobals());

describe('checkSession', () => {
  it('restores username and role from /auth/me, so role-aware UI works after a reload', async () => {
    localStorage.setItem('paqtra-token', 't');
    localStorage.setItem('paqtra-username', 'stale-name');
    const fetchMock = vi.fn(() => json(200, { username: 'bob', role: 'viewer', source: 'local' }));
    vi.stubGlobal('fetch', fetchMock);
    await useAuthStore.getState().checkSession();
    const s = useAuthStore.getState();
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/auth/me', expect.anything());
    expect(s).toMatchObject({ isAuthenticated: true, username: 'bob', role: 'viewer', authRequired: false, sessionReady: true });
  });

  it('signs out when the server rejects the token (revoked, disabled or expired)', async () => {
    localStorage.setItem('paqtra-token', 't');
    vi.stubGlobal('fetch', vi.fn(() => json(401, { error: 'Account is disabled' })));
    await useAuthStore.getState().checkSession();
    expect(useAuthStore.getState()).toMatchObject({ isAuthenticated: false, authRequired: true, sessionReady: true });
    expect(localStorage.getItem('paqtra-token')).toBeNull();
  });

  it('keeps the session on an older server without /auth/me, with an unknown role', async () => {
    localStorage.setItem('paqtra-token', 't');
    localStorage.setItem('paqtra-username', 'admin');
    vi.stubGlobal('fetch', vi.fn(() => json(404, {})));
    await useAuthStore.getState().checkSession();
    expect(useAuthStore.getState()).toMatchObject({ isAuthenticated: true, username: 'admin', role: null });
  });

  it('shows the login page without a stored token', async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);
    await useAuthStore.getState().checkSession();
    expect(fetchMock).not.toHaveBeenCalled();
    expect(useAuthStore.getState()).toMatchObject({ isAuthenticated: false, authRequired: true });
  });
});

describe('revoked sessions', () => {
  it('signs the user out when the API rejects an authenticated session', () => {
    localStorage.setItem('paqtra-token', 't');
    useAuthStore.setState({ token: 't', username: 'bob', role: 'viewer', isAuthenticated: true, authRequired: false });
    window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
    expect(useAuthStore.getState()).toMatchObject({ isAuthenticated: false, authRequired: true, role: null });
    expect(localStorage.getItem('paqtra-token')).toBeNull();
  });

  it('does nothing when nobody is signed in', () => {
    const logout = vi.fn();
    useAuthStore.setState({ isAuthenticated: false, logout });
    window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
    expect(logout).not.toHaveBeenCalled();
  });
});

describe('API 401 handling', () => {
  const reject401 = (config: InternalAxiosRequestConfig) =>
    Promise.reject(new AxiosError('Unauthorized', '401', config, undefined, { status: 401, statusText: 'Unauthorized', data: {}, headers: {}, config }));
  let heard = 0;
  const listener = () => { heard += 1; };
  const original = api.defaults.adapter;

  beforeEach(() => { heard = 0; window.addEventListener(UNAUTHORIZED_EVENT, listener); api.defaults.adapter = reject401; });
  afterEach(() => { window.removeEventListener(UNAUTHORIZED_EVENT, listener); api.defaults.adapter = original; });

  it('announces a lost session on a 401 from a protected endpoint', async () => {
    await expect(api.get('/nodes')).rejects.toBeTruthy();
    expect(heard).toBe(1);
  });

  it('does not treat a wrong password at login as a lost session', async () => {
    await expect(api.post('/auth/login', { username: 'a', password: 'b' })).rejects.toBeTruthy();
    expect(heard).toBe(0);
  });
});
