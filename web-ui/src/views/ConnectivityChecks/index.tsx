// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import {
  createConnectivityPath,
  deleteConnectivityPath,
  fetchConnectivityAlerts,
  fetchConnectivityPaths,
  fetchConnectivityPathStatus,
  silenceConnectivityAlert,
} from '../../services/api';
import { Board, Card, Eyebrow, Empty, Warning, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

type PathRow = {
  path: {
    id: string;
    name: string;
    src_namespace: string;
    src_workload: string;
    dst_namespace: string;
    dst_service: string;
    port: number;
    protocol?: string;
  };
  status: {
    status?: string;
    confidence?: string;
    notes?: string[];
    evidence_flow_ids?: string[];
    investigate?: {
      source?: { namespace?: string; name?: string };
      destination?: { namespace?: string; name?: string };
      port?: number;
    };
  };
};

type AlertRow = {
  id?: string;
  path_id?: string;
  status?: string;
  summary?: string | string[];
  notes?: string | string[];
  evidence_flow_ids?: string[];
  investigate?: PathRow['status']['investigate'];
  samples?: number;
};

export default function ConnectivityChecks() {
  const [rows, setRows] = useState<PathRow[]>([]);
  const [alerts, setAlerts] = useState<AlertRow[]>([]);
  const [err, setErr] = useState('');
  const [msg, setMsg] = useState('');
  const [name, setName] = useState('checkout→payments');
  const [srcNs, setSrcNs] = useState('default');
  const [srcWl, setSrcWl] = useState('checkout');
  const [dstNs, setDstNs] = useState('default');
  const [dstSvc, setDstSvc] = useState('payments');
  const [port, setPort] = useState('443');
  const [silencing, setSilencing] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [p, a] = await Promise.all([fetchConnectivityPaths(), fetchConnectivityAlerts()]);
      const base = ((p.data as { paths?: PathRow[] }).paths ?? []) as PathRow[];
      setRows(base);
      setAlerts(((a.data as { alerts?: AlertRow[] }).alerts ?? []) as AlertRow[]);
      setErr('');
      // Enrich status one-by-one so the list paints first.
      const enriched = await Promise.all(
        base.map(async (r) => {
          try {
            const { data } = await fetchConnectivityPathStatus(r.path.id);
            const status = (data as { status?: PathRow['status'] }).status;
            return status ? { ...r, status } : r;
          } catch {
            return r;
          }
        }),
      );
      setRows(enriched);
    } catch (e: unknown) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    const t = setInterval(() => void load(), 30_000);
    return () => clearInterval(t);
  }, [load]);

  async function add() {
    try {
      await createConnectivityPath({
        name,
        src_namespace: srcNs.trim(),
        src_workload: srcWl.trim(),
        dst_namespace: dstNs.trim(),
        dst_service: dstSvc.trim(),
        port: Number(port) || 0,
        protocol: 'TCP',
      });
      await load();
    } catch (e: unknown) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }

  async function remove(id: string) {
    try {
      await deleteConnectivityPath(id);
      await load();
    } catch (e: unknown) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }

  async function silence(alertId: string) {
    setSilencing(alertId);
    setMsg('');
    try {
      await silenceConnectivityAlert(alertId, 60);
      setMsg(`Silenced ${alertId} for 60 minutes`);
      await load();
    } catch (e: unknown) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setSilencing(null);
    }
  }

  const regressions = rows.filter((r) => r.status?.status === 'regression' || r.status?.status === 'degraded').length;
  const unknown = rows.filter((r) => r.status?.status === 'unknown').length;

  const flowsHref = (ids?: string[]) =>
    ids && ids.length > 0 ? `/flows?ids=${encodeURIComponent(ids.slice(0, 8).join(','))}` : '/flows';

  const tick = useChanged(rows);
  return (
    <Board>
      <PagePulse
        headline={tick ? (regressions ? `${regressions} path${regressions === 1 ? '' : 's'} regressed.` : rows.length ? `All ${rows.length} declared paths look healthy.` : 'No critical paths declared yet.') : undefined}
        tone={tick ? (regressions ? 'warn' : undefined) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'paths', value: tick ? rows.length : undefined },
          { label: 'regressions', value: tick ? regressions : undefined, tone: tick ? countTone(Number(regressions)) : undefined },
          { label: 'unknown', value: tick ? unknown : undefined, tone: tick ? countTone(Number(unknown)) : undefined },
          { label: 'alerts', value: tick ? alerts.length : undefined },
        ]}
      />
      <Card span={3}>
        <Eyebrow>CONNECTIVITY</Eyebrow>
        <h3>Declared service paths</h3>
        <p>
          Observe-only checks. Quiet traffic is unknown — never assumed healthy. Sustained regressions alert with
          evidence. No policy apply or BPF changes.
        </p>
      </Card>

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
        <Eyebrow>DECLARE</Eyebrow>
        <Toolbar>
          <label>
            Name
            <input value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label>
            Src ns
            <input value={srcNs} onChange={(e) => setSrcNs(e.target.value)} />
          </label>
          <label>
            Src workload
            <input value={srcWl} onChange={(e) => setSrcWl(e.target.value)} />
          </label>
          <label>
            Dst ns
            <input value={dstNs} onChange={(e) => setDstNs(e.target.value)} />
          </label>
          <label>
            Dst service
            <input value={dstSvc} onChange={(e) => setDstSvc(e.target.value)} />
          </label>
          <label>
            Port
            <input value={port} onChange={(e) => setPort(e.target.value)} />
          </label>
          <button type="button" className="primary" onClick={() => void add()}>
            Add path
          </button>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>

      <Card span={3}>
        <Eyebrow>PATHS</Eyebrow>
        {rows.length === 0 ? <Empty>No declared paths yet.</Empty> : null}
        <div className="list">
          {rows.map((r) => (
            <div className="agent wide" key={r.path.id}>
              <b>{r.path.name || r.path.id}</b>
              <span>
                {r.path.src_namespace}/{r.path.src_workload} → {r.path.dst_namespace}/{r.path.dst_service}:
                {r.path.port}
              </span>
              <small>
                {r.status?.status ?? '—'} · {r.status?.confidence ?? '—'}
              </small>
              <div style={{ display: 'flex', gap: 8, marginTop: 6, flexWrap: 'wrap' }}>
                <Link to="/investigate" state={r.status?.investigate ?? {
                  source: { namespace: r.path.src_namespace, name: r.path.src_workload },
                  destination: { namespace: r.path.dst_namespace, name: r.path.dst_service },
                  port: r.path.port,
                }}>
                  Investigate
                </Link>
                <Link to={flowsHref(r.status?.evidence_flow_ids)}>Evidence flows</Link>
                <button type="button" onClick={() => void remove(r.path.id)}>
                  Remove
                </button>
              </div>
            </div>
          ))}
        </div>
      </Card>

      <Card span={3}>
        <Eyebrow>ALERTS</Eyebrow>
        <p className="empty-state" style={{ marginBottom: 8 }}>
          Sustained multi-sample regressions only. Silence for 60 minutes during maintenance.
        </p>
        {alerts.length === 0 ? <Empty>No sustained regressions.</Empty> : null}
        <div className="list">
          {alerts.map((a, i) => {
            const id = String(a.id ?? a.path_id ?? i);
            return (
              <div className="agent wide" key={id}>
                <b>{String(a.path_id ?? a.id ?? 'alert')}</b>
                <span>{String(a.status ?? '')}{a.samples != null ? ` · ${a.samples} samples` : ''}</span>
                <small>{JSON.stringify(a.summary ?? a.notes ?? '')}</small>
                <div style={{ display: 'flex', gap: 8, marginTop: 6, flexWrap: 'wrap' }}>
                  <Link to="/investigate" state={a.investigate}>
                    Investigate
                  </Link>
                  <Link to={flowsHref(a.evidence_flow_ids)}>Evidence flows</Link>
                  <button
                    type="button"
                    disabled={silencing === id}
                    onClick={() => void silence(id)}
                  >
                    {silencing === id ? 'Silencing…' : 'Silence 60m'}
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </Card>
    </Board>
  );
}
