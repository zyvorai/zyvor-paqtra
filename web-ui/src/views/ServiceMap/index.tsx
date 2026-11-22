// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useMemo, useState } from 'react';
import { fetchServiceMap } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import NetworkGraph, { type GraphEdge, type GraphNode } from '../../components/NetworkGraph';

type Node = { id?: string; name?: string; namespace?: string };
type Edge = { source?: string; target?: string; protocol?: string };

export default function ServiceMap() {
  const [nodes, setNodes] = useState<Node[]>([]);
  const [edges, setEdges] = useState<Edge[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      const { data } = await fetchServiceMap();
      setNodes(data.nodes ?? []);
      setEdges(data.edges ?? []);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const graphNodes = useMemo<GraphNode[]>(
    () => nodes.map((n, i) => ({ id: n.id || n.name || `node-${i}`, label: n.name || n.id || `node-${i}`, group: n.namespace })),
    [nodes],
  );

  const graphEdges = useMemo<GraphEdge[]>(
    () => edges.flatMap((e) => (e.source && e.target ? [{ source: e.source, target: e.target, label: e.protocol }] : [])),
    [edges],
  );

  const tick = useChanged(edges);
  return (
    <Board>
      <PagePulse
        headline={tick ? `${nodes.length} services, ${edges.length} edges.` : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'services', value: tick ? nodes.length : undefined },
          { label: 'edges', value: tick ? edges.length : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>SERVICE MAP</Eyebrow>
        <h3>Service dependencies</h3>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
        <NetworkGraph nodes={graphNodes} edges={graphEdges} label="Service map graph" />
      </Card>
      <Card span={2}>
        <Eyebrow>SERVICES</Eyebrow>
        {nodes.length === 0 ? <Empty>No services mapped yet.</Empty> : null}
        <div className="list">
          {nodes.slice(0, 50).map((n, i) => (
            <div className="agent wide" key={n.id || i}>
              <b>{n.name || n.id || '—'}</b>
              <span>{n.namespace || '—'}</span>
            </div>
          ))}
        </div>
      </Card>
      <Card span={2}>
        <Eyebrow>EDGES</Eyebrow>
        {edges.length === 0 ? <Empty>No edges yet.</Empty> : null}
        <div className="list">
          {edges.slice(0, 50).map((e, i) => (
            <div className="agent wide" key={i}>
              <b>
                {e.source || '—'} → {e.target || '—'}
              </b>
              <span>{e.protocol || '—'}</span>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
