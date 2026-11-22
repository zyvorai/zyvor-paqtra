// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import { fetchAnomalies } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

type Anomaly = {
  id?: string;
  kind?: string;
  severity?: string;
  message?: string;
  namespace?: string;
  detected_at?: string;
};

export default function Anomalies() {
  const [items, setItems] = useState<Anomaly[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      const { data } = await fetchAnomalies();
      setItems((data as { anomalies?: Anomaly[] }).anomalies ?? []);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const tick = useChanged(items);
  return (
    <Board>
      <PagePulse
        headline={tick ? (items.length ? `${items.length} anomal${items.length === 1 ? 'y' : 'ies'} to review; nothing auto-enforces.` : 'No anomalies detected.') : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'findings', value: tick ? items.length : undefined, tone: tick ? (countTone(items.length)) : undefined },
          { label: 'high or critical', value: tick ? items.filter((x) => /crit|high/i.test(x.severity || "")).length : undefined, tone: tick ? (countTone(items.filter((x) => /crit|high/i.test(x.severity || "")).length)) : undefined },
          { label: 'types', value: tick ? new Set(items.map((x) => x.kind)).size : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>ANOMALIES</Eyebrow>
        <h3>Behavior that stands out</h3>
        <p>Review-only remediations — Paqtra does not auto-enforce.</p>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
      <Card span={3}>
        <Eyebrow>FINDINGS</Eyebrow>
        {items.length === 0 ? <Empty>No anomalies detected.</Empty> : null}
        <div className="list">
          {items.map((a, i) => (
            <div className="agent wide" key={a.id || i}>
              <b>{a.kind || a.message || `anomaly-${i}`}</b>
              <span className={`severity-badge ${(a.severity || 'info').toLowerCase()}`}>{a.severity || '—'}</span>
              <small>
                {a.namespace || '—'} · {a.detected_at ? new Date(a.detected_at).toLocaleString() : '—'}
                {a.message && a.kind ? ` · ${a.message}` : ''}
              </small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
