import { useCallback, useEffect, useState } from 'react';
import api, { apiErrorMessage } from '../../services/api';
import { formatValue, since } from '../../utils/metrics';

type Alert = {
  id: string;
  rule: string;
  node: string;
  context: string;
  chart: string;
  dimension?: string;
  status: string;
  value: number | null;
  units?: string;
  class?: string;
  info?: string;
  since: number;
  silenced?: boolean;
  acked?: boolean;
  ackedBy?: string;
};
type Transition = { time: number; id: string; rule: string; node: string; chart: string; from: string; to: string; value: number | null; units?: string; silenced?: boolean };
type Silence = { id: string; rule?: string; node?: string; chart?: string; until: number; comment?: string; createdBy?: string };
type Rule = { name: string; context: string; lookup: string; warn?: string; crit?: string; class?: string; info?: string; source: string };
type Snapshot = {
  active: Alert[];
  history: Transition[];
  rules: Rule[];
  silences: Silence[];
  stats: { rules: number; instances: number; warning: number; critical: number; evaluations: number };
};

export default function MetricAlerts() {
  const [snap, setSnap] = useState<Snapshot>();
  const [err, setErr] = useState('');
  const [note, setNote] = useState('');

  const load = useCallback(() => {
    api
      .get<Snapshot>('/metrics/alerts?history=100')
      .then((r) => {
        setSnap(r.data);
        setErr('');
      })
      .catch((e) => setErr(apiErrorMessage(e, 'metric alerts unavailable')));
  }, []);

  useEffect(() => {
    load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, [load]);

  const act = (p: Promise<unknown>, done: string) =>
    p
      .then(() => {
        setNote(done);
        load();
      })
      .catch((e) => setErr(apiErrorMessage(e, 'request failed')));

  const ack = (a: Alert) => act(api.post(`/metrics/alerts/${encodeURIComponent(a.id)}/ack`), `Acknowledged ${a.rule} on ${a.node}.`);
  const silence = (a: Alert) =>
    act(
      api.post('/metrics/silences', { rule: a.rule, node: a.node, chart: a.chart, duration: 7200, comment: 'silenced from the dashboard' }),
      `Silenced ${a.rule} on ${a.node} for 2 hours.`
    );
  const unsilence = (s: Silence) =>
    act(api.delete(`/metrics/silences/${encodeURIComponent(s.id)}`), 'Silence removed.');

  const st = snap?.stats;
  return (
    <div className="grid">
      {err && (
        <section className="card span3">
          <p className="warning">{err}</p>
        </section>
      )}
      {note && <p className="kit-caption span3">{note}</p>}
      <section className="card span3">
        <p className="eyebrow">RAISED</p>
        <h2 className="card-title">
          {st ? `${st.critical} critical · ${st.warning} warning` : 'Loading…'}
          {st && <span className="kit-caption"> — {st.rules} rules over {st.instances} instances</span>}
        </h2>
        {snap && snap.active.length === 0 && <p className="empty-state">Nothing raised. Every evaluated instance is clear.</p>}
        {snap && snap.active.length > 0 && (
          <table className="metric-table">
            <thead>
              <tr>
                <th>Status</th>
                <th>Alert</th>
                <th>Node</th>
                <th>Chart</th>
                <th className="num">Value</th>
                <th>For</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {snap.active.map((a) => (
                <tr key={a.id}>
                  <td>
                    <span className={`metric-status ${a.status}`}>{a.status}</span>
                  </td>
                  <td>
                    <b>{a.rule}</b>
                    {a.info && <div className="kit-caption">{a.info}</div>}
                    {a.silenced && <div className="kit-caption">silenced</div>}
                    {a.acked && <div className="kit-caption">acknowledged{a.ackedBy ? ` by ${a.ackedBy}` : ''}</div>}
                  </td>
                  <td>{a.node}</td>
                  <td>
                    {a.chart}
                    {a.dimension ? ` / ${a.dimension}` : ''}
                  </td>
                  <td className="num">{formatValue(a.value, a.units)}</td>
                  <td>{since(a.since)}</td>
                  <td>
                    {!a.acked && (
                      <button type="button" className="btn-secondary" onClick={() => ack(a)}>
                        Ack
                      </button>
                    )}
                    {!a.silenced && (
                      <button type="button" className="btn-secondary" onClick={() => silence(a)}>
                        Silence 2h
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>
      {snap && snap.silences.length > 0 && (
        <section className="card span3">
          <p className="eyebrow">SILENCES</p>
          <table className="metric-table">
            <thead>
              <tr>
                <th>Rule</th>
                <th>Node</th>
                <th>Chart</th>
                <th>Until</th>
                <th>By</th>
                <th>Comment</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {snap.silences.map((s) => (
                <tr key={s.id}>
                  <td>{s.rule || '*'}</td>
                  <td>{s.node || '*'}</td>
                  <td>{s.chart || '*'}</td>
                  <td>{new Date(s.until * 1000).toLocaleString()}</td>
                  <td>{s.createdBy}</td>
                  <td>{s.comment}</td>
                  <td>
                    <button type="button" className="btn-secondary" onClick={() => unsilence(s)}>
                      Remove
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
      <section className="card span3">
        <p className="eyebrow">HISTORY</p>
        <table className="metric-table">
          <thead>
            <tr>
              <th>When</th>
              <th>Alert</th>
              <th>Node</th>
              <th>Chart</th>
              <th>Change</th>
              <th className="num">Value</th>
            </tr>
          </thead>
          <tbody>
            {(snap?.history || []).map((h, i) => (
              <tr key={`${h.id}-${h.time}-${i}`}>
                <td>{new Date(h.time * 1000).toLocaleTimeString()}</td>
                <td>{h.rule}</td>
                <td>{h.node}</td>
                <td>{h.chart}</td>
                <td>
                  <span className={`metric-status ${h.from}`}>{h.from}</span> → <span className={`metric-status ${h.to}`}>{h.to}</span>
                  {h.silenced && <span className="kit-caption"> (silenced)</span>}
                </td>
                <td className="num">{formatValue(h.value, h.units)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
      <section className="card span3">
        <details>
          <summary>
            {snap?.rules.length || 0} rules loaded (built-in pack plus PAQTRA_METRICALERT_DIR)
          </summary>
          <table className="metric-table">
            <thead>
              <tr>
                <th>Rule</th>
                <th>Context</th>
                <th>Lookup</th>
                <th>Warn</th>
                <th>Crit</th>
                <th>Source</th>
              </tr>
            </thead>
            <tbody>
              {(snap?.rules || []).map((r) => (
                <tr key={r.name} title={r.info}>
                  <td>{r.name}</td>
                  <td>{r.context}</td>
                  <td>{r.lookup}</td>
                  <td>
                    <code>{r.warn}</code>
                  </td>
                  <td>
                    <code>{r.crit}</code>
                  </td>
                  <td>{r.source}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </details>
      </section>
    </div>
  );
}
