// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useEffect, useState } from 'react';
import { useAuthStore } from '../stores/authStore';

const WRONG = 'Wrong username or password.';

export default function LoginPage() {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const login = useAuthStore((s) => s.login);
  const host = window.location.host || window.location.hostname;

  useEffect(() => {
    const saved = localStorage.getItem('paqtra-username');
    if (saved) setUsername(saved);
  }, []);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!username.trim() || !password) {
      setError(WRONG);
      return;
    }
    setBusy(true);
    setError('');
    try {
      await login(username.trim(), password);
    } catch (e: unknown) {
      const msg =
        e && typeof e === 'object' && 'response' in e
          ? (e as { response?: { data?: { error?: string } } }).response?.data?.error
          : undefined;
      setError(msg || WRONG);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="login-shell">
      <div className="login-info">
        <img src="/zyvor-mark.svg" alt="Zyvor" className="login-logo" />
        <p className="eyebrow">Paqtra · Zyvor</p>
        <h1>Trace every flow.</h1>
        <p>
          See where network traffic goes and why it is allowed or dropped — full-stack observability for Kubernetes
          networks powered by Cilium eBPF.
        </p>
        <p className="login-host">
          Connecting to <code>{host}</code>
        </p>
      </div>
      <form className="card login-card" onSubmit={submit} noValidate>
        <h1>Sign in.</h1>
        <label className="tokenbox">
          Username
          <input
            value={username}
            onChange={(e) => {
              setUsername(e.target.value);
              if (error) setError('');
            }}
            autoFocus
            autoComplete="username"
            disabled={busy}
            aria-invalid={Boolean(error)}
          />
        </label>
        <label className="tokenbox">
          Password
          <input
            type="password"
            value={password}
            onChange={(e) => {
              setPassword(e.target.value);
              if (error) setError('');
            }}
            autoComplete="current-password"
            disabled={busy}
            aria-invalid={Boolean(error)}
          />
        </label>
        {error ? (
          <p className="login-error" role="alert" aria-live="assertive">
            {error}
          </p>
        ) : null}
        <button type="submit" className="primary" disabled={busy}>
          {busy ? 'Signing in…' : 'Sign in'}
        </button>
        <p className="login-hint">
          Lab default: <code>admin</code> / <code>Admin@321</code>
        </p>
      </form>
    </div>
  );
}
