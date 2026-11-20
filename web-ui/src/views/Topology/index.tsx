// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useMemo, useState } from 'react';
import { fetchServiceDeps } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import NetworkGraph, { type GraphEdge, type GraphNode } from '../../components/NetworkGraph';

type Dep = { source?: string; destination?: string; namespace?: string; protocol?: string; bytes?: number };

export default function Topology() {
  const [deps, setDeps] = useState<Dep[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      setDeps(((await fetchServiceDeps()).data as { dependencies?: Dep[] }).dependencies ?? []);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const nodes = new Set(deps.flatMap((d) => [d.source, d.destination].filter(Boolean))).size;

  const graphNodes = useMemo<GraphNode[]>(() => {
    const byId = new Map<string, GraphNode>();
    for (const d of deps) {
      if (d.source && !byId.has(d.source)) byId.set(d.source, { id: d.source, label: d.source, group: d.namespace });
      if (d.destination && !byId.has(d.destination)) byId.set(d.destination, { id: d.destination, label: d.destination, group: d.namespace });
    }
    return Array.from(byId.values());
  }, [deps]);

  const graphEdges = useMemo<GraphEdge[]>(
    () => deps.flatMap((d) => (d.source && d.destination ? [{ source: d.source, target: d.destination, label: d.protocol }] : [])),
    [deps],
  );

  const tick = useChanged(deps);
  return (
    <Board>
      <PagePulse
        headline={tick ? `${nodes} services, ${deps.length} observed dependencies.` : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'edges', value: tick ? deps.length : undefined },
          { label: 'nodes', value: tick ? nodes : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>TOPOLOGY</Eyebrow>
        <h3>Observed dependency graph</h3>
        <p>Force-directed layout of edges from observed traffic — drag a node to pin it.</p>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
        <NetworkGraph nodes={graphNodes} edges={graphEdges} label="Service dependency graph" />
      </Card>
      <Card span={3}>
        <Eyebrow>EDGES</Eyebrow>
        <h3>Source → destination</h3>
        {deps.length === 0 ? <Empty>No topology edges yet.</Empty> : null}
        <div className="list">
          {deps.slice(0, 80).map((d, i) => (
            <div className="agent wide" key={i}>
              <b>
                {d.source || '—'} → {d.destination || '—'}
              </b>
              <span>{d.protocol || '—'}</span>
              <small>
                {d.namespace || '—'} · {(d.bytes ?? 0).toLocaleString()} B
              </small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
