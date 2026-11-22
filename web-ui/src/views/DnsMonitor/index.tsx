// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import { fetchDnsQueries, fetchDnsStats } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

type Query = {
  query_name?: string;
  namespace?: string;
  source_pod?: string;
  response_code?: string;
  rcode?: string;
  latency_ms?: number;
  response_ips?: string[];
  confidence?: string;
  evidence?: string;
  verdict?: string;
  policy_correlation?: { confidence?: string; detail?: string | null; drop_reason?: string };
};

type Stats = {
  total?: number;
  total_queries?: number;
  failures?: number;
  avg_latency_ms?: number;
  l7_observed?: number;
  l4_only?: number;
  source?: string;
  [key: string]: unknown;
};

export default function DnsMonitor() {
  const [queries, setQueries] = useState<Query[]>([]);
  const [stats, setStats] = useState<Stats | null>(null);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      const [q, s] = await Promise.all([fetchDnsQueries(), fetchDnsStats()]);
      setQueries((q.data as { queries?: Query[] }).queries ?? []);
      setStats(s.data as Stats);
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

  const tick = useChanged(queries);
  return (
    <Board>
      <PagePulse
        headline={tick ? ((stats?.failures ?? 0) ? `${stats?.failures} DNS failures observed.` : 'DNS answers are clean.') : undefined}
        tone={tick ? ((stats?.failures ?? 0) ? 'warn' : undefined) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'queries', value: tick ? stats?.total ?? stats?.total_queries ?? queries.length : undefined },
          { label: 'DNS failures', value: tick ? stats?.failures ?? 0 : undefined, tone: tick ? countTone(Number(stats?.failures ?? 0)) : undefined },
          { label: 'L7 observed', value: tick ? stats?.l7_observed ?? 0 : undefined },
          { label: 'avg latency ms', value: tick ? stats?.avg_latency_ms ?? '—' : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>DNS PULSE</Eyebrow>
        {stats?.source ? <p style={{ opacity: 0.7, fontSize: 13 }}>{stats.source}</p> : null}
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
      <Card span={3}>
        <Eyebrow>RECENT</Eyebrow>
        <h3>Observed DNS</h3>
        {queries.length === 0 ? <Empty>No DNS queries yet.</Empty> : null}
        <div className="datatable-scroll">
          <div
            className="datahead"
            style={{ gridTemplateColumns: '1.6fr 1fr 0.9fr 0.7fr 0.7fr 1.2fr' }}
          >
            <span>QUERY</span>
            <span>POD</span>
            <span>RCODE</span>
            <span>LATENCY</span>
            <span>EVIDENCE</span>
            <span>POLICY</span>
          </div>
          {queries.slice(0, 80).map((q, i) => (
            <div
              className="datarow"
              key={i}
              style={{ gridTemplateColumns: '1.6fr 1fr 0.9fr 0.7fr 0.7fr 1.2fr' }}
            >
              <span className="truncate">{q.query_name || '(L4 only — no query name)'}</span>
              <span className="truncate">
                {q.namespace ? `${q.namespace}/` : ''}
                {q.source_pod || '—'}
              </span>
              <span>{q.response_code || q.rcode || '—'}</span>
              <span>{q.latency_ms != null && q.latency_ms > 0 ? `${q.latency_ms} ms` : '—'}</span>
              <span>{q.evidence || q.confidence || '—'}</span>
              <span className="truncate">
                {q.policy_correlation?.detail ||
                  (q.verdict === 'DROPPED' ? 'dropped' : '—')}
              </span>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
