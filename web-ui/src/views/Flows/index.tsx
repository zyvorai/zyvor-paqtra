// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import api, { fetchFlows, fetchFlowStats, type Flow, type FlowEndpoint } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import TerminalFrame from '../../components/TerminalFrame';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

function endpointLabel(ep: FlowEndpoint | string | undefined): string {
  if (!ep) return '—';
  if (typeof ep === 'string') return ep;
  const ns = ep.namespace?.trim();
  const name = (ep.pod || ep.ip || '').trim();
  if (ns && name) return `${ns}/${name}`;
  if (name) return name;
  if (ns) return ns;
  return '—';
}

function flowLine(f: Flow): string {
  const src = endpointLabel(f.source);
  const dst = endpointLabel(f.destination);
  const port = f.port ? `:${f.port}` : '';
  const t = f.timestamp ? new Date(f.timestamp).toLocaleTimeString() : '';
  return `${t}  ${f.verdict || '—'}  ${f.protocol || '—'}  ${src} → ${dst}${port}`;
}

type ExplainResult = {
  id?: string;
  likely_owner?: string;
  steps?: { title: string; detail: string; confidence: string }[];
  next_actions?: string[];
};

export default function Flows() {
  const [flows, setFlows] = useState<Flow[]>([]);
  const [stats, setStats] = useState<{
    total?: number;
    forwarded?: number;
    dropped?: number;
    total_flows?: number;
  } | null>(null);
  const [verdict, setVerdict] = useState('');
  const [namespace, setNamespace] = useState('');
  const [err, setErr] = useState('');
  const [explain, setExplain] = useState<ExplainResult | null>(null);
  const [explaining, setExplaining] = useState(false);

  const load = useCallback(async () => {
    try {
      const [f, s] = await Promise.all([
        fetchFlows({
          limit: 100,
          ...(verdict ? { verdict } : {}),
          ...(namespace ? { namespace } : {}),
        }),
        fetchFlowStats(),
      ]);
      setFlows(f.data.flows ?? []);
      setStats(s.data as typeof stats);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, [verdict, namespace]);

  useEffect(() => {
    void load();
    const t = setInterval(() => void load(), 5000);
    return () => clearInterval(t);
  }, [load]);

  const whyDenied = async (f: Flow) => {
    setExplaining(true);
    setExplain(null);
    try {
      const { data } = await api.post<ExplainResult>('/investigate/flow', {
        flow_id: f.id,
        flow: f,
      });
      setExplain(data);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setExplaining(false);
    }
  };

  const forwarded = stats?.forwarded ?? 0;
  const dropped = stats?.dropped ?? 0;
  const total = stats?.total ?? stats?.total_flows ?? flows.length;

  const tick = useChanged(stats);
  return (
    <Board>
      <PagePulse
        headline={tick ? (dropped ? `${dropped.toLocaleString()} of ${total.toLocaleString()} sampled flows dropped.` : `${total.toLocaleString()} flows sampled, none dropped.`) : undefined}
        tone={tick ? (dropped ? 'warn' : undefined) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'flows sampled', value: tick ? total : undefined },
          { label: 'forwarded', value: tick ? forwarded : undefined },
          { label: 'dropped', value: tick ? dropped : undefined, tone: tick ? countTone(Number(dropped)) : undefined },
          { label: 'rows shown', value: tick ? flows.length : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}

      <Card span={3}>
        <Eyebrow>LIVE STREAM</Eyebrow>
        <h3>Filters</h3>
        <p>
          Optional Cilium/Hubble enrichment. Select a DROPPED row to open the deny explanation
          workflow.
        </p>
        <Toolbar>
          <label>
            Verdict
            <input value={verdict} placeholder="FORWARDED" onChange={(e) => setVerdict(e.target.value)} />
          </label>
          <label>
            Namespace
            <input value={namespace} placeholder="default" onChange={(e) => setNamespace(e.target.value)} />
          </label>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>


      <div className="span3">
        <TerminalFrame title="hubble.GetFlows / filtered">
          {flows.length === 0 ? (
            <Empty>No flows yet — waiting on Hubble Relay.</Empty>
          ) : (
            <div style={{ fontSize: 12, lineHeight: 1.55, fontFamily: 'ui-monospace, monospace' }}>
              {flows.slice(0, 120).map((f) => (
                <div key={f.id} style={{ display: 'flex', gap: 8, alignItems: 'baseline' }}>
                  <span style={{ flex: 1 }}>{flowLine(f)}</span>
                  {String(f.verdict).toUpperCase() === 'DROPPED' ? (
                    <button
                      type="button"
                      className="btn-refresh"
                      style={{ fontSize: 11, padding: '2px 8px' }}
                      disabled={explaining}
                      onClick={() => void whyDenied(f)}
                    >
                      Why denied?
                    </button>
                  ) : null}
                </div>
              ))}
            </div>
          )}
        </TerminalFrame>
      </div>

      {explain ? (
        <Card span={3}>
          <Eyebrow>WHY DENIED</Eyebrow>
          <h3>Owner: {explain.likely_owner || 'unknown'}</h3>
          <ol style={{ margin: '0.5rem 0', paddingLeft: '1.2rem' }}>
            {(explain.steps ?? []).map((s, i) => (
              <li key={i} style={{ marginBottom: 8 }}>
                <strong>{s.title}</strong>{' '}
                <span style={{ opacity: 0.65 }}>({s.confidence})</span>
                <div style={{ whiteSpace: 'pre-wrap', fontSize: 13 }}>{s.detail}</div>
              </li>
            ))}
          </ol>
          {(explain.next_actions ?? []).length > 0 ? (
            <>
              <Eyebrow>NEXT</Eyebrow>
              <ul>
                {(explain.next_actions ?? []).map((a, i) => (
                  <li key={i}>{a}</li>
                ))}
              </ul>
            </>
          ) : null}
        </Card>
      ) : null}
    </Board>
  );
}
