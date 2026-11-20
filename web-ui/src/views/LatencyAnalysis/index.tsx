// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react';
import { fetchLatencyAnalysis } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

type Row = {
  service?: string;
  namespace?: string;
  p50_ms?: number;
  p99_ms?: number;
  avg_ms?: number;
};

export default function LatencyAnalysis() {
  const [rows, setRows] = useState<Row[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      setRows(((await fetchLatencyAnalysis()).data as { services?: Row[] }).services ?? []);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const worst = [...rows].sort((a, b) => (b.p99_ms ?? 0) - (a.p99_ms ?? 0))[0];

  const tick = useChanged(rows);
  return (
    <Board>
      <PagePulse
        headline={tick ? (worst ? `${worst.service} has the highest p99 at ${worst.p99_ms ?? '—'} ms.` : 'No latency samples yet.') : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'services', value: tick ? rows.length : undefined },
          { label: 'highest p99', value: tick ? worst?.service ?? '—' : undefined, tone: tick ? countTone(Number(worst?.service ?? '—')) : undefined },
          { label: 'p99 ms', value: tick ? worst?.p99_ms ?? '—' : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>PATH</Eyebrow>
        <h3>Connect latency and pressure</h3>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
      <Card span={3}>
        <Eyebrow>BREAKDOWN</Eyebrow>
        {rows.length === 0 ? <Empty>No latency samples yet.</Empty> : null}
        <div className="datatable-scroll">
          <div className="datahead" style={{ gridTemplateColumns: '2fr 1fr 0.8fr 0.8fr 0.8fr' }}>
            <span>SERVICE</span>
            <span>NS</span>
            <span>P50</span>
            <span>AVG</span>
            <span>P99</span>
          </div>
          {rows.slice(0, 80).map((r, i) => (
            <div className="datarow" key={i} style={{ gridTemplateColumns: '2fr 1fr 0.8fr 0.8fr 0.8fr' }}>
              <span className="truncate">{r.service || '—'}</span>
              <span>{r.namespace || '—'}</span>
              <span>{r.p50_ms ?? '—'}</span>
              <span>{r.avg_ms ?? '—'}</span>
              <span>{r.p99_ms ?? '—'}</span>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
