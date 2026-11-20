// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react';
import { fetchIdentities, type CiliumIdentity } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

export default function Identities() {
  const [items, setItems] = useState<CiliumIdentity[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      setItems((await fetchIdentities()).data.identities ?? []);
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
        headline={tick ? `${items.length} security identities in the Cilium identity map.` : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'identities', value: tick ? items.length : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}

      <Card span={3}>
        <Eyebrow>IDENTITIES</Eyebrow>
        <h3>Security identities</h3>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>

      <Card span={3}>
        <Eyebrow>MAP</Eyebrow>
        <h3>Identity → labels</h3>
        {items.length === 0 ? <Empty>No identities reported yet.</Empty> : null}
        <div className="list">
          {items.slice(0, 100).map((id) => (
            <div className="agent wide" key={id.id}>
              <b>{id.id}</b>
              <span>{id.namespace || '—'}</span>
              <small>{(id.labels || []).slice(0, 6).join(', ') || '—'}</small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
