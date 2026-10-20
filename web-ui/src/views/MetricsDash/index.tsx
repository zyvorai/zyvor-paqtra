import { useCallback, useEffect, useState } from 'react';
import { fetchMetricsSummary, fetchCiliumStatus, fetchEbpfSummary } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { rateTone } from '../../components/kit/tone';

export default function MetricsDash() {
  const [metrics, setMetrics] = useState<Record<string, unknown> | null>(null);
  const [agents, setAgents] = useState(0);
  const [ebpf, setEbpf] = useState<Record<string, unknown> | null>(null);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      const [m, c, e] = await Promise.allSettled([
        fetchMetricsSummary(),
        fetchCiliumStatus(),
        fetchEbpfSummary(),
      ]);
      if (m.status === 'fulfilled') setMetrics(m.value.data as Record<string, unknown>);
      if (c.status === 'fulfilled') setAgents(((c.value.data as { agents?: unknown[] }).agents ?? []).length);
      if (e.status === 'fulfilled') setEbpf(e.value.data as Record<string, unknown>);
      const failed = [m, c, e].find((r) => r.status === 'rejected') as PromiseRejectedResult | undefined;
      setErr(failed ? String(failed.reason) : '');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    const t = setInterval(() => void load(), 10000);
    return () => clearInterval(t);
  }, [load]);

  const tick = useChanged(metrics);
  return (
    <Board>
      <PagePulse
        headline={tick ? `${agents} agent${agents === 1 ? '' : 's'} reporting to the scorecard.` : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'req/s', value: tick ? typeof metrics?.requests_per_sec === 'number' ? Number(metrics.requests_per_sec).toFixed(1) : '—' : undefined },
          { label: 'avg latency ms', value: tick ? typeof metrics?.avg_latency_ms === 'number' ? Number(metrics.avg_latency_ms).toFixed(1) : '—' : undefined },
          { label: 'error rate', value: tick ? typeof metrics?.error_rate === 'number'
                ? `${(Number(metrics.error_rate) * 100).toFixed(2)}%`
                : '—' : undefined, tone: tick ? rateTone(metrics?.error_rate) : undefined },
          { label: 'Cilium agents', value: tick ? agents : undefined },
          { label: 'BPF programs', value: tick ? Number(ebpf?.programs_total ?? ebpf?.programs ?? 0) || '—' : undefined },
          { label: 'BPF maps', value: tick ? Number(ebpf?.maps_total ?? ebpf?.maps ?? 0) || '—' : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}

      <Card span={3}>
        <Eyebrow>SCORECARD</Eyebrow>
        <h3>One board for the shift</h3>
        {!metrics && !err ? <Empty>Waiting for metrics…</Empty> : null}
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
    </Board>
  );
}
