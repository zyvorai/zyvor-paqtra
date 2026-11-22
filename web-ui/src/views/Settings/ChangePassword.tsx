// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useEffect, useState } from 'react';
import { fetchMe, changePassword, apiErrorMessage } from '../../services/api';
import { useAuthStore } from '../../stores/authStore';
import { Card, Eyebrow, Warning, Toolbar } from '../../components/Board';

/** Self-service password change, for any signed-in local user. */
export default function ChangePassword() {
  const [source, setSource] = useState<string | null>(null);
  const [current, setCurrent] = useState('');
  const [next, setNext] = useState('');
  const [confirm, setConfirm] = useState('');
  const [err, setErr] = useState('');
  const [done, setDone] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    fetchMe().then((r) => setSource(r.data.source)).catch(() => setSource(null));
  }, []);

  // The configured admin's password is an environment setting, and with auth
  // disabled there is no account: nothing to change here.
  if (source === 'config' || source === 'auth_disabled') {
    return (
      <Card>
        <Eyebrow>ACCOUNT</Eyebrow>
        <h3>Password</h3>
        <p>
          {source === 'config'
            ? <>This account&apos;s password is set by <code>ADMIN_PASSWORD</code> in the server configuration.</>
            : 'Authentication is disabled on this server.'}
        </p>
      </Card>
    );
  }

  const mismatch = confirm.length > 0 && confirm !== next;

  const submit = async () => {
    setSaving(true);
    setErr('');
    try {
      await changePassword({ current_password: current, new_password: next });
      setDone(true);
      // Every token issued before the change is now refused, this session's included.
      setTimeout(() => useAuthStore.getState().logout(), 1500);
    } catch (e) {
      setErr(apiErrorMessage(e, 'Could not change password'));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Card>
      <Eyebrow>ACCOUNT</Eyebrow>
      <h3>Change password</h3>
      {err ? <Warning>{err}</Warning> : null}
      {done ? (
        <p className="empty-state">Password changed. Signing you out so you can sign in again…</p>
      ) : (
        <Toolbar>
          <label>
            Current password
            <input type="password" value={current} onChange={(e) => setCurrent(e.target.value)} autoComplete="current-password" />
          </label>
          <label>
            New password (12+ characters)
            <input type="password" value={next} onChange={(e) => setNext(e.target.value)} autoComplete="new-password" />
          </label>
          <label>
            Confirm new password
            <input type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} autoComplete="new-password" aria-invalid={mismatch} />
          </label>
          <button type="button" className="primary" disabled={saving || !current || !next || next !== confirm} onClick={submit}>
            Change password
          </button>
          {mismatch ? <small>Passwords do not match.</small> : null}
        </Toolbar>
      )}
    </Card>
  );
}
