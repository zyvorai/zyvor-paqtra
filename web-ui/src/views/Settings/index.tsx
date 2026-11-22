// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { checkHealth } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Toolbar } from '../../components/Board';
import { applyTheme, readStoredTheme, toggleTheme, type Theme } from '../../theme';
import ChangePassword from './ChangePassword';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

interface AppSettings {
  apiUrl: string;
  hubbleAddress: string;
  refreshInterval: number;
}

const STORAGE_KEY = 'paqtra-settings';
const DEFAULTS: AppSettings = {
  apiUrl: '/api/v1',
  hubbleAddress: 'hubble-relay.kube-system.svc.cluster.local:4245',
  refreshInterval: 10,
};

function loadSettings(): AppSettings {
  try {
    const s = localStorage.getItem(STORAGE_KEY);
    return s ? { ...DEFAULTS, ...JSON.parse(s) } : { ...DEFAULTS };
  } catch {
    return { ...DEFAULTS };
  }
}

export default function Settings() {
  const [settings, setSettings] = useState<AppSettings>(loadSettings);
  const [msg, setMsg] = useState('');
  const [err, setErr] = useState('');
  const [apiStatus, setApiStatus] = useState<'checking' | 'connected' | 'disconnected'>('checking');
  const [theme, setTheme] = useState<Theme>(() => readStoredTheme());

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  useEffect(() => {
    checkHealth()
      .then(() => setApiStatus('connected'))
      .catch(() => setApiStatus('disconnected'));
  }, []);

  const save = () => {
    const url = settings.apiUrl.trim();
    if (url.startsWith('/')) {
      /* ok */
    } else if (url.startsWith('https://')) {
      try {
        if (new URL(url).origin !== window.location.origin) {
          setErr('API URL must be same-origin.');
          return;
        }
      } catch {
        setErr('Invalid API URL.');
        return;
      }
    } else if (url.startsWith('http://')) {
      setErr('Use https:// or a relative path like /api/v1 — plain HTTP is not allowed.');
      return;
    } else {
      setErr('API URL must start with / or https://.');
      return;
    }
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
    setErr('');
    setMsg('Saved.');
  };

  const tick = useChanged(apiStatus);
  return (
    <Board>
      <PagePulse
        headline={tick ? `API ${apiStatus}.` : undefined}
        tone={tick ? (apiStatus === 'disconnected' ? 'bad' : undefined) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'API', value: tick ? apiStatus : undefined, tone: tick ? ((apiStatus === 'connected' ? 'ok' : apiStatus === 'disconnected' ? 'bad' : undefined)) : undefined },
          { label: 'theme', value: tick ? theme : undefined },
          { label: 'refresh s', value: tick ? settings.refreshInterval : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      {msg ? (
        <Card span={3}>
          <p className="empty-state">{msg}</p>
        </Card>
      ) : null}

      <Card span={2}>
        <Eyebrow>CONNECTION</Eyebrow>
        <h3>API and Hubble</h3>
        <p>
          Prefer relative <code>/api/v1</code> so the UI proxy keeps everything on HTTPS (same-origin).
        </p>
        <Toolbar>
          <label>
            API URL
            <input
              value={settings.apiUrl}
              onChange={(e) => setSettings({ ...settings, apiUrl: e.target.value })}
            />
          </label>
          <label>
            Hubble address
            <input
              value={settings.hubbleAddress}
              onChange={(e) => setSettings({ ...settings, hubbleAddress: e.target.value })}
            />
          </label>
          <label>
            Refresh (s)
            <input
              type="number"
              value={settings.refreshInterval}
              onChange={(e) => setSettings({ ...settings, refreshInterval: Number(e.target.value) || 10 })}
            />
          </label>
          <button type="button" className="primary" onClick={save}>
            Save
          </button>
        </Toolbar>
        <p>
          API status: <b>{apiStatus}</b>
        </p>
      </Card>

      <Card>
        <Eyebrow>APPEARANCE</Eyebrow>
        <h3>Theme</h3>
        <Toolbar>
          <button
            type="button"
            className="btn-refresh"
            onClick={() => setTheme((t) => toggleTheme(t))}
          >
            Switch to {theme === 'dark' ? 'light' : 'dark'}
          </button>
        </Toolbar>
      </Card>

      <ChangePassword />

      <Card span={3}>
        <Eyebrow>MORE</Eyebrow>
        <h3>Deep links outside the mega-nav</h3>
        <p>Netra keeps the top nav focused. These remain reachable by bookmark:</p>
        <div className="chips">
          <Link to="/chaos">Chaos</Link>
          <Link to="/replay">Replay</Link>
          <Link to="/healer">Healer</Link>
          <Link to="/bgp">BGP</Link>
          <Link to="/ipam">IPAM</Link>
          <Link to="/clustermesh">ClusterMesh</Link>
          <Link to="/alerts">Alerts</Link>
          <Link to="/costs">Costs</Link>
        </div>
      </Card>
    </Board>
  );
}
