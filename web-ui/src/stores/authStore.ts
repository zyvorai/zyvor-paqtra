// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { create } from 'zustand';
import api, { UNAUTHORIZED_EVENT } from '../services/api';

const TOKEN_KEY = 'paqtra-token';
const USER_KEY = 'paqtra-username';

interface AuthState {
  token: string | null;
  username: string | null;
  role: string | null;
  isAuthenticated: boolean;
  /** True until the first session probe finishes (Netra-style gate). */
  sessionReady: boolean;
  /** Show the login page when true. */
  authRequired: boolean;
  login: (username: string, password: string) => Promise<void>;
  logout: () => void;
  checkSession: () => Promise<void>;
}

function clearAuthHeaders() {
  delete api.defaults.headers.common['Authorization'];
}

export const useAuthStore = create<AuthState>((set) => ({
  token: localStorage.getItem(TOKEN_KEY),
  username: localStorage.getItem(USER_KEY),
  role: null,
  isAuthenticated: false,
  sessionReady: false,
  authRequired: true,

  login: async (username: string, password: string) => {
    const response = await api.post('/auth/login', { username, password });
    const { token, role } = response.data;
    localStorage.setItem(TOKEN_KEY, token);
    localStorage.setItem(USER_KEY, username);
    api.defaults.headers.common['Authorization'] = `Bearer ${token}`;
    set({
      token,
      username,
      role: role ?? 'viewer',
      isAuthenticated: true,
      authRequired: false,
      sessionReady: true,
    });
  },

  logout: () => {
    localStorage.removeItem(TOKEN_KEY);
    localStorage.removeItem(USER_KEY);
    clearAuthHeaders();
    set({
      token: null,
      username: null,
      role: null,
      isAuthenticated: false,
      authRequired: true,
      sessionReady: true,
    });
  },

  checkSession: async () => {
    const token = localStorage.getItem(TOKEN_KEY);
    if (!token) {
      clearAuthHeaders();
      set({
        token: null,
        isAuthenticated: false,
        authRequired: true,
        sessionReady: true,
      });
      return;
    }

    api.defaults.headers.common['Authorization'] = `Bearer ${token}`;
    try {
      // /auth/me validates the token against the account as it is now (a disabled
      // or deleted user is refused) and tells us the current role. /health is
      // auth-exempt, so it cannot be used to probe.
      const resp = await fetch('/api/v1/auth/me', {
        headers: {
          Authorization: `Bearer ${token}`,
          Accept: 'application/json',
        },
      });
      if (resp.status === 401) {
        localStorage.removeItem(TOKEN_KEY);
        clearAuthHeaders();
        set({
          token: null,
          isAuthenticated: false,
          authRequired: true,
          sessionReady: true,
        });
        return;
      }
      let me: { username?: string; role?: string } = {};
      if (resp.ok) {
        try { me = await resp.json(); } catch { /* keep the stored username */ }
      }
      set({
        token,
        username: me.username ?? localStorage.getItem(USER_KEY),
        role: me.role ?? null,
        isAuthenticated: true,
        authRequired: false,
        sessionReady: true,
      });
    } catch {
      // API unreachable — still show login so the gate is visible.
      set({
        isAuthenticated: false,
        authRequired: true,
        sessionReady: true,
      });
    }
  },
}));

// The API refused the session (revoked, expired, account disabled): sign out.
if (typeof window !== 'undefined') {
  window.addEventListener(UNAUTHORIZED_EVENT, () => {
    if (useAuthStore.getState().isAuthenticated) useAuthStore.getState().logout();
  });
}
