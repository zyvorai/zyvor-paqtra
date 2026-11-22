// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useRef, useState } from 'react';
import {
  fetchFlowHistory, fetchFlowTimeline, apiErrorMessage,
  FlowHistoryResponse, FlowTimelineResponse, FlowHistoryParams,
} from '../../services/api';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import TimelineChart from './TimelineChart';
import { PRESETS, RangePreset, formatCount, rangeFor } from './timeline';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

const PAGE_SIZE = 50;
const VERDICTS = ['', 'FORWARDED', 'DROPPED', 'AUDIT', 'ERROR'];

type Filters = Omit<FlowHistoryParams, 'limit' | 'offset'>;

const when = (iso: string | null | undefined): string => (iso ? new Date(iso).toLocaleString() : 'unknown');
const endpoint = (e: { namespace: string; pod: string; ip: string }): string => {
  const name = e.pod || e.ip || 'unknown';
  return e.namespace ? `${e.namespace}/${name}` : name;
};

export default function FlowHistory() {
  // Draft filters, edited freely; nothing is fetched until Search.
  const [preset, setPreset] = useState<RangePreset>('1h');
  const [custom, setCustom] = useState({ from: '', to: '' });
  const [namespace, setNamespace] = useState('');
  const [pod, setPod] = useState('');
  const [port, setPort] = useState('');
  const [verdict, setVerdict] = useState('');

  const [applied, setApplied] = useState<Filters | null>(null);
  const [timeline, setTimeline] = useState<FlowTimelineResponse | null>(null);
  const [page, setPage] = useState<FlowHistoryResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState('');
  // Ignore a response that arrives after a newer search has started.
  const latest = useRef(0);

  const search = useCallback(async () => {
    const range = rangeFor(preset, new Date(), custom);
    if ('error' in range) {
      setErr(range.error);
      return;
    }
    const portNum = port.trim() === '' ? undefined : Number(port);
    if (portNum !== undefined && (!Number.isInteger(portNum) || portNum < 1 || portNum > 65535)) {
      setErr('Port must be a number from 1 to 65535.');
      return;
    }
    const filters: Filters = {
      ...range,
      namespace: namespace.trim() || undefined,
      pod: pod.trim() || undefined,
      port: portNum,
      verdict: verdict || undefined,
    };
    const id = ++latest.current;
    setLoading(true);
    setErr('');
    try {
      const [t, h] = await Promise.all([
        fetchFlowTimeline(filters),
        fetchFlowHistory({ ...filters, limit: PAGE_SIZE, offset: 0 }),
      ]);
      if (id !== latest.current) return;
      setApplied(filters);
      setTimeline(t.data);
      setPage(h.data);
    } catch (e) {
      if (id === latest.current) setErr(apiErrorMessage(e, 'Failed to load flow history'));
    } finally {
      if (id === latest.current) setLoading(false);
    }
  }, [preset, custom, namespace, pod, port, verdict]);

  const goTo = async (offset: number) => {
    if (!applied) return;
    const id = ++latest.current;
    setLoading(true);
    setErr('');
    try {
      // The same range and filters as the chart: the page never re-reads "now".
      const h = await fetchFlowHistory({ ...applied, limit: PAGE_SIZE, offset });
      if (id === latest.current) setPage(h.data);
    } catch (e) {
      if (id === latest.current) setErr(apiErrorMessage(e, 'Failed to load flows'));
    } finally {
      if (id === latest.current) setLoading(false);
    }
  };

  // First load on mount only: no polling, so a result under study is never replaced.
  useAutoRefresh(search, 30000, false);

  const cov = page?.coverage ?? timeline?.coverage;
  const cap = page?.capture ?? timeline?.capture;
  const droppedTotal = timeline ? timeline.buckets.reduce((n, b) => n + b.dropped, 0) : 0;
  const rangeSecs = timeline ? (new Date(timeline.range.to).getTime() - new Date(timeline.range.from).getTime()) / 1000 : 0;
  const first = page ? page.offset + 1 : 0;
  const last = page ? page.offset + page.flows.length : 0;

  const tick = useChanged(timeline);
  return (
    <Board>
      <PagePulse
        headline={tick ? (timeline ? (droppedTotal ? `${droppedTotal.toLocaleString()} of ${timeline.total.toLocaleString()} stored flows dropped.` : `${timeline.total.toLocaleString()} stored flows, none dropped.`) : undefined) : undefined}
        tone={tick ? (droppedTotal ? 'warn' : undefined) : undefined}
        tick={tick}
        live={false}
        error={err || undefined}
        figures={[
          { label: 'flows found', value: tick ? timeline?.total ?? 0 : undefined },
          { label: 'dropped', value: tick ? droppedTotal : undefined, tone: tick ? (countTone(droppedTotal)) : undefined },
          { label: 'per bar', value: tick && timeline ? (timeline.bucket_secs >= 3600 ? `${timeline.bucket_secs / 3600} h` : `${timeline.bucket_secs / 60} min`) : undefined },
        ]}
      />
      <Card span={3}>
        <Eyebrow>FLOWS</Eyebrow>
        <h3>Flow history</h3>
        <p>
          What the flow store captured over time: search a range, filter it, and see when traffic was forwarded or dropped. Flows are
          captured by polling Hubble, so read counts as lower bounds.
        </p>

        {/* One filter row, above everything it scopes. */}
        <Toolbar>
          <label>
            Range
            <select value={preset} onChange={(e) => setPreset(e.target.value as RangePreset)}>
              {PRESETS.map((p) => <option key={p.id} value={p.id}>{p.label}</option>)}
              <option value="custom">Custom range…</option>
            </select>
          </label>
          {preset === 'custom' ? (
            <>
              <label>
                From
                <input type="datetime-local" value={custom.from} onChange={(e) => setCustom({ ...custom, from: e.target.value })} />
              </label>
              <label>
                To
                <input type="datetime-local" value={custom.to} onChange={(e) => setCustom({ ...custom, to: e.target.value })} />
              </label>
            </>
          ) : null}
          <label>
            Namespace
            <input value={namespace} onChange={(e) => setNamespace(e.target.value)} placeholder="either side" autoComplete="off" />
          </label>
          <label>
            Pod
            <input value={pod} onChange={(e) => setPod(e.target.value)} placeholder="name contains" autoComplete="off" />
          </label>
          <label>
            Port
            <input value={port} onChange={(e) => setPort(e.target.value)} inputMode="numeric" placeholder="any" style={{ width: '6rem' }} />
          </label>
          <label>
            Verdict
            <select value={verdict} onChange={(e) => setVerdict(e.target.value)}>
              {VERDICTS.map((v) => <option key={v} value={v}>{v || 'All verdicts'}</option>)}
            </select>
          </label>
          {/* Not disabled while loading: a new search supersedes a slow one, and the stale response is dropped. */}
          <button type="button" className="primary" onClick={() => void search()}>
            {loading ? 'Searching…' : 'Search'}
          </button>
        </Toolbar>
      </Card>

      {err ? <Card span={3}><Warning>{err}</Warning></Card> : null}

      {cap && cov ? (
        <Card span={3}>
          {!cov.durable ? (
            <Warning>Flow history is held in memory: it is lost when the API restarts and is capped in size. Set PAQTRA_DATA_DIR to keep it.</Warning>
          ) : null}
          {!cap.last_capture_ok ? (
            <Warning>The last flow capture did not succeed{cap.last_capture_at ? ` (${when(cap.last_capture_at)})` : ''}, so recent flows may be missing.</Warning>
          ) : null}
          {cov.range_starts_before_oldest ? (
            <p>
              Stored history begins {when(cov.oldest)}. Part of this range is earlier than that: nothing from before then was captured, or it has aged
              out (flows are kept {cov.retention_days} days).
            </p>
          ) : null}
          <details>
            <summary>How complete is this?</summary>
            <p>{cap.note}</p>
          </details>
        </Card>
      ) : null}

      {timeline && page ? (
        <>
          <Card span={3}>
            <Eyebrow>OVER TIME</Eyebrow>
            {timeline.total === 0 ? (
              <Empty>No stored flows match this range and these filters. Try a wider range, or check when history begins above.</Empty>
            ) : (
              <TimelineChart buckets={timeline.buckets} bucketSecs={timeline.bucket_secs} rangeSecs={rangeSecs} loading={loading} />
            )}
          </Card>

          {page.total > 0 ? (
            <Card span={3}>
              <Eyebrow>FLOWS</Eyebrow>
              <div className="viz-table-wrap" style={{ maxHeight: 'none', opacity: loading ? 0.5 : 1 }}>
                <table className="viz-table">
                  <thead>
                    <tr>
                      <th>Time</th>
                      <th>Verdict</th>
                      <th>Source</th>
                      <th>Destination</th>
                      <th className="num">Port</th>
                    </tr>
                  </thead>
                  <tbody>
                    {page.flows.map((f) => (
                      <tr key={`${f.id}-${f.timestamp}`}>
                        <td>{when(f.timestamp)}</td>
                        <td className="strong">{f.verdict}</td>
                        <td>{endpoint(f.source)}</td>
                        <td>{endpoint(f.destination)}</td>
                        <td className="num">{f.protocol} {f.port}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <Toolbar>
                <button type="button" className="btn-refresh" disabled={loading || page.offset === 0} onClick={() => void goTo(Math.max(0, page.offset - PAGE_SIZE))}>
                  Previous
                </button>
                <span>
                  {formatCount(first)}–{formatCount(last)} of {formatCount(page.total)}
                </span>
                <button type="button" className="btn-refresh" disabled={loading || last >= page.total} onClick={() => void goTo(page.offset + PAGE_SIZE)}>
                  Next
                </button>
              </Toolbar>
            </Card>
          ) : null}
        </>
      ) : null}
    </Board>
  );
}
