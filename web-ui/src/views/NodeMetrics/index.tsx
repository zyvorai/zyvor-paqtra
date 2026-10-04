import { useEffect, useMemo, useState } from 'react';
import api, { apiErrorMessage } from '../../services/api';
import MetricChart from '../../components/MetricChart';
import { useMetricStream } from '../../hooks/useMetricStream';
import { chartsFor, familyOf, groupContexts, windows, type ContextInfo } from '../../utils/metrics';

type NodeInfo = { node: string; lastIngest: string; stats?: { series?: number } };

export default function NodeMetrics() {
  const [nodes, setNodes] = useState<NodeInfo[]>([]);
  const [contexts, setContexts] = useState<ContextInfo[]>([]);
  const [node, setNode] = useState('');
  const [family, setFamily] = useState('');
  const [win, setWin] = useState(300);
  const [split, setSplit] = useState('dimension');
  const [filter, setFilter] = useState('');
  const [err, setErr] = useState('');
  const [highlight, setHighlight] = useState<{ id: string; range: [number, number] } | null>(null);

  useEffect(() => {
    const load = () => {
      api
        .get<{ nodes: NodeInfo[] }>('/metrics/nodes')
        .then((r) => setNodes(r.data.nodes || []))
        .catch((e) => setErr(apiErrorMessage(e, 'metric nodes unavailable')));
      api
        .get<{ contexts: ContextInfo[] }>('/metrics/contexts')
        .then((r) => {
          setContexts(r.data.contexts || []);
          setErr('');
        })
        .catch((e) => setErr(apiErrorMessage(e, 'metric contexts unavailable')));
    };
    load();
    const t = setInterval(load, 30000);
    return () => clearInterval(t);
  }, []);

  const visible = useMemo(() => {
    const f = filter.trim().toLowerCase();
    return contexts.filter((c) => (!node || c.charts.some((ch) => ch.node === node)) && (!f || c.context.toLowerCase().includes(f) || (c.title || '').toLowerCase().includes(f)));
  }, [contexts, node, filter]);
  const groups = useMemo(() => groupContexts(visible), [visible]);
  const fam = family && visible.some((c) => familyOf(c) === family) ? family : groups[0]?.families[0]?.family || '';
  const famContexts = useMemo(() => visible.filter((c) => familyOf(c) === fam), [visible, fam]);
  const charts = useMemo(() => chartsFor(famContexts, node, node ? 'dimension' : split, win), [famContexts, node, split, win]);
  const queries = useMemo(() => charts.map((c) => c.q), [charts]);
  const { results, live, error } = useMetricStream(queries);

  return (
    <div className="grid">
      {(err || error) && (
        <section className="card span3">
          <p className="warning">{err || error}</p>
        </section>
      )}
      <div className="span3">
        <div className="metrics-toolbar">
          <select value={node} onChange={(e) => setNode(e.target.value)} aria-label="Node">
            <option value="">All nodes ({nodes.length})</option>
            {nodes.map((n) => (
              <option key={n.node} value={n.node}>
                {n.node}
              </option>
            ))}
          </select>
          <div className="metrics-seg" role="group" aria-label="Time window">
            {windows.map((w) => (
              <button type="button" key={w.label} className={win === w.seconds ? 'active' : ''} onClick={() => setWin(w.seconds)}>
                {w.label}
              </button>
            ))}
          </div>
          {!node && (
            <div className="metrics-seg" role="group" aria-label="Split fleet charts by">
              {['dimension', 'node'].map((s) => (
                <button type="button" key={s} className={split === s ? 'active' : ''} onClick={() => setSplit(s)}>
                  by {s}
                </button>
              ))}
            </div>
          )}
          <input placeholder="Filter metrics…" value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Filter metrics" />
          <span className={`metrics-live${live ? ' on' : ''}`}>{live ? 'live · 1s' : win > 3600 ? 'polling' : 'connecting…'}</span>
        </div>
        {contexts.length === 0 && !err && <p className="empty-state">No metrics yet. Agents stream per-second metrics once PAQTRA_METRICS is on (the default) and the agent can reach the API with PAQTRA_AGENT_KEY.</p>}
        {contexts.length > 0 && (
          <div className="metrics-layout">
            <nav className="card metrics-side" aria-label="Metric families">
              {groups.map((g) => (
                <div key={g.section}>
                  <h3>{g.section}</h3>
                  {g.families.map((f) => (
                    <button type="button" key={f.family} className={f.family === fam ? 'active' : ''} onClick={() => setFamily(f.family)}>
                      {f.family}
                      <small>{f.contexts.length}</small>
                    </button>
                  ))}
                </div>
              ))}
            </nav>
            <div className="metrics-charts">
              {charts.map((c) => (
                <section className="card" key={c.q.id}>
                  <MetricChart
                    result={results[c.q.id]}
                    title={c.title}
                    subtitle={c.subtitle}
                    highlight={highlight?.id === c.q.id ? highlight.range : undefined}
                    onHighlight={(after, before) => setHighlight({ id: c.q.id, range: [after, before] })}
                  />
                </section>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
