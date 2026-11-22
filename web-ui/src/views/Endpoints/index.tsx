// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import { fetchEndpoints, type CiliumEndpoint } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

export default function Endpoints() {
  const [endpoints, setEndpoints] = useState<CiliumEndpoint[]>([]);
  const [err, setErr] = useState('');
  const [search, setSearch] = useState('');

  const load = useCallback(async () => {
    try {
      setEndpoints((await fetchEndpoints()).data.endpoints ?? []);
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

  const filtered = search
    ? endpoints.filter(
        (e) =>
          e.name?.toLowerCase().includes(search.toLowerCase()) ||
          e.namespace?.toLowerCase().includes(search.toLowerCase()),
      )
    : endpoints;
  const ready = endpoints.filter((e) => (e.status || '').toLowerCase() === 'ready').length;

  const tick = useChanged(endpoints);
  return (
    <Board>
      <PagePulse
        headline={tick ? (endpoints.length - ready ? `${endpoints.length - ready} of ${endpoints.length} endpoints not ready.` : `All ${endpoints.length} endpoints ready.`) : undefined}
        tone={tick ? (endpoints.length - ready ? 'warn' : undefined) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'endpoints', value: tick ? endpoints.length : undefined },
          { label: 'ready', value: tick ? ready : undefined },
          { label: 'namespaces', value: tick ? new Set(endpoints.map((e) => e.namespace)).size : undefined },
          { label: 'not ready', value: tick ? endpoints.length - ready : undefined, tone: tick ? (countTone(endpoints.length - ready)) : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}

      <Card span={3}>
        <Eyebrow>ENDPOINTS</Eyebrow>
        <h3>Cilium-managed workloads</h3>
        <Toolbar>
          <label>
            Search
            <input value={search} placeholder="name or namespace" onChange={(e) => setSearch(e.target.value)} />
          </label>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>

      <Card span={3}>
        <Eyebrow>INVENTORY</Eyebrow>
        <h3>Endpoint list</h3>
        {filtered.length === 0 ? <Empty>No endpoints reported yet.</Empty> : null}
        <div className="list">
          {filtered.slice(0, 100).map((e) => (
            <div className="agent wide" key={`${e.namespace}/${e.name}/${e.id ?? e.identity}`}>
              <b>
                {e.namespace}/{e.name}
              </b>
              <span>{e.status || '—'}</span>
              <small>
                identity {e.identity ?? '—'} · { (e.labels || []).slice(0, 3).join(', ') || 'no labels'}
              </small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
