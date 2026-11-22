// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import { fetchCiliumStatus } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

type Agent = {
  name?: string;
  node?: string;
  status?: string;
  version?: string;
  message?: string;
};

export default function CiliumStatus() {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      setAgents(((await fetchCiliumStatus()).data as { agents?: Agent[] }).agents ?? []);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    const t = setInterval(() => void load(), 15000);
    return () => clearInterval(t);
  }, [load]);

  const ok = agents.filter((a) => (a.status || '').toLowerCase().includes('ok') || (a.status || '').toLowerCase() === 'ready').length;

  const tick = useChanged(agents);
  return (
    <Board>
      <PagePulse
        headline={tick ? (agents.length - ok ? `${agents.length - ok} of ${agents.length} Cilium agents not healthy.` : `All ${agents.length} Cilium agents healthy.`) : undefined}
        tone={tick ? (agents.length - ok ? 'warn' : undefined) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'agents', value: tick ? agents.length : undefined },
          { label: 'healthy', value: tick ? ok : undefined },
          { label: 'other', value: tick ? agents.length - ok : undefined, tone: tick ? (countTone(agents.length - ok)) : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}

      <Card span={3}>
        <Eyebrow>CILIUM</Eyebrow>
        <h3>Agent status</h3>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>

      <Card span={3}>
        <Eyebrow>PER NODE</Eyebrow>
        <h3>Datapath brotherhood</h3>
        <p>Paqtra observes these agents — it never replaces Cilium’s datapath.</p>
        {agents.length === 0 ? <Empty>No Cilium agents reported yet.</Empty> : null}
        <div className="list">
          {agents.map((a, i) => (
            <div className="agent wide" key={a.name || a.node || i}>
              <b>{a.node || a.name || `agent-${i}`}</b>
              <span>{a.status || '—'}</span>
              <small>
                {a.version || '—'} {a.message ? `· ${a.message}` : ''}
              </small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
