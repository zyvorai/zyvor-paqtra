// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import { fetchNodes, type K8sNode } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

export default function Nodes() {
  const [nodes, setNodes] = useState<K8sNode[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      setNodes((await fetchNodes()).data.nodes ?? []);
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

  const totalCpu = nodes.reduce((a, n) => a + (n.cpu_capacity ?? 0), 0);
  const totalMem = nodes.reduce((a, n) => a + (n.memory_capacity_gb ?? 0), 0);
  const totalPods = nodes.reduce((a, n) => a + (n.pods_count ?? n.pods ?? 0), 0);
  const ready = nodes.filter((n) => (n.status || '').toLowerCase() === 'ready' || (n.status || '').toLowerCase() === 'true').length;

  const tick = useChanged(nodes);
  return (
    <Board>
      <PagePulse
        headline={tick ? (nodes.length - ready ? `${nodes.length - ready} of ${nodes.length} nodes not ready.` : (nodes.length === 1 ? 'The node is ready.' : `All ${nodes.length} nodes ready.`)) : undefined}
        tone={tick ? (nodes.length - ready ? 'warn' : undefined) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'nodes', value: tick ? nodes.length : undefined },
          { label: 'ready', value: tick ? ready : undefined },
          { label: 'CPU cores', value: tick ? totalCpu : undefined },
          { label: 'memory GB', value: tick ? Math.round(totalMem) : undefined },
          { label: 'pods', value: tick ? totalPods : undefined },
          { label: 'not ready', value: tick ? nodes.length - ready : undefined, tone: tick ? (countTone(nodes.length - ready)) : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}

      <Card span={3}>
        <Eyebrow>FLEET</Eyebrow>
        <h3>Every node, one glance</h3>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>

      <Card span={3}>
        <Eyebrow>INVENTORY</Eyebrow>
        <h3>Per-node</h3>
        {nodes.length === 0 ? <Empty>No nodes reported yet.</Empty> : null}
        <div className="list">
          {nodes.map((n) => (
            <div className="agent wide" key={n.name}>
              <b>{n.name}</b>
              <span>{(n.status || '').toLowerCase() === 'ready' ? 'ready' : n.status || '—'}</span>
              <small>
                CPU {n.cpu_usage ?? 0}/{n.cpu_capacity ?? 0} · mem {(n.memory_usage_gb ?? 0).toFixed?.(1) ?? n.memory_usage_gb}/
                {Math.round((n.memory_capacity_gb ?? 0) * 10) / 10} GB · pods {n.pods_count ?? n.pods ?? 0}
              </small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
