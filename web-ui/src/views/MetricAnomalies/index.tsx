import { useEffect, useMemo, useState } from 'react';
import api, { apiErrorMessage } from '../../services/api';
import MetricChart from '../../components/MetricChart';
import { useMetricStream } from '../../hooks/useMetricStream';
import { formatValue, streamId, timelineResult, windows, type AnomalySummary as Summary, type Ranked } from '../../utils/metrics';

export default function MetricAnomalies() {
  const [win, setWin] = useState(3600);
  const [data, setData] = useState<Summary>();
  const [correlated, setCorrelated] = useState<Ranked[]>();
  const [hl, setHl] = useState<[number, number] | undefined>();
  const [pick, setPick] = useState<Ranked | undefined>();
  const [err, setErr] = useState('');

  useEffect(() => {
    const load = () => {
      api
        .get<{ summary: Summary }>(`/metrics/anomalies?${new URLSearchParams({ after: String(-win), top: '50' })}`)
        .then((r) => {
          setData(r.data.summary);
          setErr('');
        })
        .catch((e) => setErr(apiErrorMessage(e, 'anomalies unavailable')));
      if (hl) {
        api
          .get<{ ranked: Ranked[] | null }>(`/metrics/correlations?${new URLSearchParams({ after: String(hl[0]), before: String(hl[1]), top: '50' })}`)
          .then((r) => setCorrelated(r.data.ranked || []))
          .catch((e) => setErr(apiErrorMessage(e, 'correlations unavailable')));
      } else {
        setCorrelated(undefined);
      }
    };
    load();
    const t = setInterval(load, hl ? 60000 : 10000);
    return () => clearInterval(t);
  }, [win, hl]);

  const timeline = useMemo(() => (data ? timelineResult(data) : undefined), [data]);
  const rows = hl ? correlated || [] : data?.ranked || [];
  const pickQuery = useMemo(
    () => (pick ? [{ id: streamId(pick.context, pick.node, pick.chart), context: pick.context, charts: [pick.chart], nodes: [pick.node], window: win, points: 300 }] : []),
    [pick, win]
  );
  const { results } = useMetricStream(pickQuery);

  return (
    <div className="grid">
      {err && (
        <section className="card span3">
          <p className="warning">{err}</p>
        </section>
      )}
      <div className="span3 metrics-toolbar">
        <div className="metrics-seg" role="group" aria-label="Time window">
          {windows.slice(1, 6).map((w) => (
            <button
              type="button"
              key={w.label}
              className={win === w.seconds ? 'active' : ''}
              onClick={() => {
                setWin(w.seconds);
                setHl(undefined);
              }}
            >
              {w.label}
            </button>
          ))}
        </div>
        {hl ? (
          <button type="button" className="btn-secondary" onClick={() => setHl(undefined)}>
            Clear highlight
          </button>
        ) : (
          <span className="kit-caption">Drag across the timeline to ask “what changed here?”</span>
        )}
      </div>
      <section className="card span3">
        <MetricChart result={timeline} title="Anomaly rate per node" subtitle="share of samples flagged by the per-dimension models" height={180} onHighlight={(a, b) => setHl([a, b])} highlight={hl} />
      </section>
      <section className="card span3">
        <p className="eyebrow">NODES</p>
        <table className="metric-table">
          <thead>
            <tr>
              <th>Node</th>
              <th>Anomaly rate</th>
              <th className="num">Anomalous / scored dimensions</th>
            </tr>
          </thead>
          <tbody>
            {(data?.nodes || []).map((n) => (
              <tr key={n.node}>
                <td>{n.node}</td>
                <td>
                  <div className="metric-bar" title={formatValue(n.anomalyRate, '%')}>
                    <span style={{ width: `${Math.min(100, n.anomalyRate * 5)}%` }} />
                  </div>
                  {formatValue(n.anomalyRate, '%')}
                </td>
                <td className="num">
                  {n.anomalousDimensions} / {n.dimensions}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        {data && !(data.nodes || []).length && <p className="empty-state">No scored metrics in this window yet.</p>}
      </section>
      <section className="card span3">
        <p className="eyebrow">{hl ? 'WHAT CHANGED IN THE HIGHLIGHT' : 'MOST ANOMALOUS'}</p>
        <h2 className="card-title">{hl ? 'Ranked by distribution shift versus the preceding baseline (Kolmogorov–Smirnov)' : 'Dimensions ranked by anomaly rate'}</h2>
        <table className="metric-table">
          <thead>
            <tr>
              <th>#</th>
              <th>Node</th>
              <th>Chart</th>
              <th>Dimension</th>
              <th className="num">Anomaly rate</th>
              <th className="num">Score</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r, i) => (
              <tr key={`${r.node}|${r.chart}|${r.dimension}`} className="clickable" onClick={() => setPick(r)}>
                <td>{i + 1}</td>
                <td>{r.node}</td>
                <td>{r.chart}</td>
                <td>{r.dimension}</td>
                <td className="num">{formatValue(r.anomalyRate, '%')}</td>
                <td className="num">{r.score.toFixed(2)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {data && rows.length === 0 && <p className="empty-state">{hl ? 'Nothing changed noticeably in that window.' : 'No anomalous dimensions in this window.'}</p>}
      </section>
      {pick && (
        <section className="card span3">
          <MetricChart result={results[pickQuery[0].id]} title={pick.chart} subtitle={`${pick.node} · ${pick.context}`} highlight={hl} />
        </section>
      )}
    </div>
  );
}
