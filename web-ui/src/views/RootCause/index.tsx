// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { fetchPacketDrops } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

type Drop = {
  reason?: string;
  source?: string;
  destination?: string;
  count?: number;
  suggestion?: string;
};

export default function RootCause() {
  const [drops, setDrops] = useState<Drop[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      setDrops(((await fetchPacketDrops()).data as { drops?: Drop[] }).drops ?? []);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const tick = useChanged(drops);
  return (
    <Board>
      <PagePulse
        headline={tick ? (drops.length ? `${drops.length} drop finding${drops.length === 1 ? '' : 's'} to explain.` : 'No drop findings to explain.') : undefined}
        tone={tick ? (drops.length ? 'warn' : undefined) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'findings', value: tick ? drops.length : undefined, tone: tick ? (countTone(drops.length)) : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>ROOT CAUSE</Eyebrow>
        <h3>Why it dropped</h3>
        <p>
          Correlates drops with policy evidence. Raw drop counts live on <Link to="/drops">Drops</Link>.
        </p>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
      <Card span={3}>
        <Eyebrow>FINDINGS</Eyebrow>
        {drops.length === 0 ? <Empty>No root-cause findings yet.</Empty> : null}
        <div className="list">
          {drops.slice(0, 60).map((d, i) => (
            <div className="agent wide" key={i}>
              <b>{d.reason || 'unknown'}</b>
              <span>{d.count ?? 0}</span>
              <small>
                {d.source || '—'} → {d.destination || '—'}
                {d.suggestion ? ` · ${d.suggestion}` : ''}
              </small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
