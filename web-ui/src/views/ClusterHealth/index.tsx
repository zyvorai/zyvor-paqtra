// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react';
import {
  fetchClusterHealth,
  fetchFlowStore,
  purgeFlowStore,
  type ClusterHealthSummary,
} from '../../services/api';
import { Board, Card, Eyebrow, Metric, Metrics, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { scoreTone } from '../../components/kit/tone';

type FlowStoreInfo = {
  retention_days?: number;
  coverage?: { from?: string; to?: string; count?: number };
  ingest?: {
    connected?: boolean;
    gaps?: number;
    last_gap_at?: string | null;
    recent_gaps?: string[];
    events_per_sec?: number;
    lag_secs?: number;
    source?: string;
  };
};

export default function ClusterHealth() {
  const [health, setHealth] = useState<ClusterHealthSummary | null>(null);
  const [store, setStore] = useState<FlowStoreInfo | null>(null);
  const [err, setErr] = useState('');
  const [msg, setMsg] = useState('');
  const [purgeNs, setPurgeNs] = useState('');
  const [purgeDays, setPurgeDays] = useState('7');
  const [purging, setPurging] = useState(false);

  const load = useCallback(async () => {
    try {
      const [h, s] = await Promise.all([
        fetchClusterHealth(),
        fetchFlowStore().catch(() => null),
      ]);
      setHealth(h.data);
      if (s) setStore(s.data as FlowStoreInfo);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    const t = setInterval(() => void load(), 10000);
    return () => clearInterval(t);
  }, [load]);

  async function doPurge() {
    if (!window.confirm('Purge old flow store rows? This cannot be undone.')) return;
    setPurging(true);
    setMsg('');
    try {
      const { data } = await purgeFlowStore({
        namespace: purgeNs.trim() || undefined,
        older_than_days: Number(purgeDays) || undefined,
      });
      setMsg(`Purged ${String((data as { deleted?: number }).deleted ?? 0)} rows`);
      await load();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setPurging(false);
    }
  }

  const score = health?.score ?? health?.health_score ?? 0;
  const status = health?.overall ?? health?.status ?? '—';
  const gaps = store?.ingest?.recent_gaps ?? [];

  const tick = useChanged(health);
  return (
    <Board>
      <PagePulse
        headline={tick ? `Cluster ${status} · score ${score}/100.` : undefined}
        tone={tick ? scoreTone(score) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'health score /100', value: tick ? score : undefined, tone: tick ? (scoreTone(Number(score))) : undefined },
          { label: 'status', value: tick ? status : undefined },
          { label: 'nodes', value: tick ? health?.node_count ?? health?.kubernetes?.nodes ?? 0 : undefined },
          { label: 'pods', value: tick ? health?.pod_count ?? health?.kubernetes?.pods ?? 0 : undefined },
          { label: 'endpoints', value: tick ? health?.endpoint_count ?? health?.kubernetes?.endpoints ?? 0 : undefined },
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

      <Card span={3}>
        <Eyebrow>HEALTH PULSE</Eyebrow>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>

      <Card span={3}>
        <Eyebrow>FLOW STORE</Eyebrow>
        <h3>Retention & ingest coverage</h3>
        {!store ? (
          <Empty>Flow store info unavailable.</Empty>
        ) : (
          <>
            <Metrics>
              <Metric value={store.retention_days ?? '—'} label="retention days" />
              <Metric value={store.ingest?.gaps ?? 0} label="gap count" />
              <Metric value={Math.round((store.ingest?.events_per_sec ?? 0) * 10) / 10} label="events/s" />
              <Metric value={store.ingest?.connected ? 'yes' : 'no'} label="stream connected" />
            </Metrics>
            <p className="empty-state" style={{ marginTop: 8 }}>
              Source: {store.ingest?.source ?? '—'}
              {store.coverage?.from
                ? ` · coverage ${store.coverage.from} → ${store.coverage.to ?? 'now'} (${store.coverage.count ?? 0})`
                : ''}
              {store.ingest?.last_gap_at ? ` · last gap ${store.ingest.last_gap_at}` : ''}
            </p>
            <Toolbar>
              <label>
                Namespace (optional)
                <input value={purgeNs} onChange={(e) => setPurgeNs(e.target.value)} placeholder="all" />
              </label>
              <label>
                Older than days
                <input value={purgeDays} onChange={(e) => setPurgeDays(e.target.value)} />
              </label>
              <button type="button" disabled={purging} onClick={() => void doPurge()}>
                {purging ? 'Purging…' : 'Purge (admin)'}
              </button>
            </Toolbar>
          </>
        )}
      </Card>

      <Card span={3}>
        <Eyebrow>INGEST GAP TIMELINE</Eyebrow>
        <h3>Recent capture gaps</h3>
        {gaps.length === 0 ? <Empty>No recent gaps recorded.</Empty> : null}
        <div className="list">
          {gaps.map((g) => (
            <div className="agent wide" key={g}>
              <b>gap</b>
              <small>{g}</small>
            </div>
          ))}
        </div>
      </Card>

      <Card span={3}>
        <Eyebrow>COMPONENTS</Eyebrow>
        <h3>Subsystem status</h3>
        {!health ? <Empty>Waiting for cluster health…</Empty> : null}
        {(health?.components ?? []).length === 0 && health ? (
          <Empty>No component details yet.</Empty>
        ) : null}
        <div className="list">
          {(health?.components ?? []).map((c) => (
            <div className="agent wide" key={c.name}>
              <b>{c.name}</b>
              <span className={`severity-badge ${c.status === 'healthy' ? 'info' : c.status === 'degraded' ? 'warning' : 'critical'}`}>
                {c.status}
              </span>
              <small>{c.message || '—'}</small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
