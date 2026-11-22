// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import { fetchCaptureSessions } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

type Session = {
  id?: string;
  node?: string;
  status?: string;
  filter?: string;
  started_at?: string;
};

export default function PacketCapture() {
  const [sessions, setSessions] = useState<Session[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      setSessions(((await fetchCaptureSessions()).data as { sessions?: Session[] }).sessions ?? []);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const active = sessions.filter((s) => (s.status || '').toLowerCase() === 'running' || (s.status || '').toLowerCase() === 'active').length;

  const tick = useChanged(sessions);
  return (
    <Board>
      <PagePulse
        headline={tick ? (active ? `${active} capture${active === 1 ? '' : 's'} running.` : 'No capture running.') : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'sessions', value: tick ? sessions.length : undefined },
          { label: 'active', value: tick ? active : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>CAPTURE</Eyebrow>
        <h3>Watch the wire, live</h3>
        <p>Filtered, time-bounded captures — observe-only; never affects the datapath verdict.</p>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
      <Card span={3}>
        <Eyebrow>SESSIONS</Eyebrow>
        {sessions.length === 0 ? <Empty>No capture sessions yet.</Empty> : null}
        <div className="list">
          {sessions.map((s, i) => (
            <div className="agent wide" key={s.id || i}>
              <b>{s.node || s.id || `session-${i}`}</b>
              <span>{s.status || '—'}</span>
              <small>
                {s.filter || 'no filter'} · {s.started_at ? new Date(s.started_at).toLocaleString() : '—'}
              </small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
