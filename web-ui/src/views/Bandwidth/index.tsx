// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import { fetchBandwidthData } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

type Entry = {
  name?: string;
  namespace?: string;
  bytes_in?: number;
  bytes_out?: number;
  packets?: number;
  [key: string]: unknown;
};

export default function Bandwidth() {
  const [entries, setEntries] = useState<Entry[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      setEntries(((await fetchBandwidthData()).data as { entries?: Entry[] }).entries ?? []);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const ranked = [...entries].sort(
    (a, b) => (b.bytes_out ?? 0) + (b.bytes_in ?? 0) - ((a.bytes_out ?? 0) + (a.bytes_in ?? 0)),
  );

  const tick = useChanged(entries);
  return (
    <Board>
      <PagePulse
        headline={tick ? (ranked[0] ? `${ranked[0].name ?? ranked[0].namespace} is the top destination by bytes.` : 'No byte counts yet.') : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'entries', value: tick ? entries.length : undefined },
          { label: 'top talker', value: tick ? ranked[0]?.name ?? ranked[0]?.namespace ?? '—' : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}

      <Card span={3}>
        <Eyebrow>TALKERS</Eyebrow>
        <h3>Who is talking the most</h3>
        <p>Top destinations by byte count. No payloads.</p>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>

      <Card span={3}>
        <Eyebrow>RANKING</Eyebrow>
        <h3>By bytes</h3>
        {ranked.length === 0 ? <Empty>No talker data yet.</Empty> : null}
        <div className="list">
          {ranked.slice(0, 40).map((e, i) => (
            <div className="agent wide" key={`${e.name}-${i}`}>
              <b>{e.name || e.namespace || `talker-${i}`}</b>
              <span>{((e.bytes_in ?? 0) + (e.bytes_out ?? 0)).toLocaleString()} B</span>
              <small>
                in {(e.bytes_in ?? 0).toLocaleString()} · out {(e.bytes_out ?? 0).toLocaleString()}
              </small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
