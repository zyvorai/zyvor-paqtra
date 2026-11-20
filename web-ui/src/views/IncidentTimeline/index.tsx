// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react';
import { fetchIncidents } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

type Incident = {
  id?: string;
  title?: string;
  severity?: string;
  status?: string;
  started_at?: string;
  summary?: string;
  affected_services?: string[];
  duration?: string;
  root_cause?: string;
};

export default function IncidentTimeline() {
  const [items, setItems] = useState<Incident[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      setItems(((await fetchIncidents()).data as { incidents?: Incident[] }).incidents ?? []);
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
        headline={tick ? (items.length ? `${items.length} incident${items.length === 1 ? '' : 's'} on the timeline.` : 'No incidents recorded.') : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'incidents', value: tick ? items.length : undefined, tone: tick ? (countTone(items.length)) : undefined },
          { label: 'high or critical', value: tick ? items.filter((x) => /crit|high/i.test(x.severity || "")).length : undefined, tone: tick ? (countTone(items.filter((x) => /crit|high/i.test(x.severity || "")).length)) : undefined },
          { label: 'open', value: tick ? items.filter((x) => !/resolv|clos/i.test(x.status || "")).length : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>INCIDENTS</Eyebrow>
        <h3>When signals agree</h3>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
      <Card span={3}>
        <Eyebrow>TIMELINE</Eyebrow>
        {items.length === 0 ? <Empty>No incidents joined yet.</Empty> : null}
        <div className="list">
          {items.map((x, i) => (
            <div className="agent wide" key={x.id || i}>
              <b>{x.title || x.id || `incident-${i}`}</b>
              <span className={`severity-badge ${(x.severity || 'info').toLowerCase()}`}>{x.severity || x.status || '—'}</span>
              <small>
                {x.started_at ? new Date(x.started_at).toLocaleString() : '—'}
                {x.status ? ` · ${x.status}${x.duration ? ` after ${x.duration}` : ''}` : ''}
                {x.summary ? ` · ${x.summary}` : ''}
                {x.affected_services?.length ? ` · affects ${x.affected_services.join(', ')}` : ''}
                {x.root_cause ? ` · root cause: ${x.root_cause}` : ''}
              </small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
