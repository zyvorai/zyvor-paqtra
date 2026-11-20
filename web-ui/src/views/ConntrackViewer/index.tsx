// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react';
import { fetchEbpfConntrack } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

type Entry = {
  src?: string;
  dst?: string;
  proto?: string;
  state?: string;
  packets?: number;
};

export default function ConntrackViewer() {
  const [entries, setEntries] = useState<Entry[]>([]);
  const [total, setTotal] = useState(0);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      const { data } = await fetchEbpfConntrack();
      setEntries((data.entries as Entry[]) ?? []);
      setTotal(data.total ?? 0);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const tick = useChanged(entries);
  return (
    <Board>
      <PagePulse
        headline={tick ? `${(total || entries.length).toLocaleString()} conntrack entries, read-only.` : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'entries', value: tick ? total || entries.length : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>CONNTRACK</Eyebrow>
        <h3>Read-only CT table</h3>
        <p>Paqtra reads Cilium conntrack — it never writes the map.</p>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
      <Card span={3}>
        <Eyebrow>ENTRIES</Eyebrow>
        {entries.length === 0 ? <Empty>No conntrack entries visible.</Empty> : null}
        <div className="datatable-scroll">
          <div className="datahead" style={{ gridTemplateColumns: '1.4fr 1.4fr 0.6fr 0.8fr' }}>
            <span>SRC</span>
            <span>DST</span>
            <span>PROTO</span>
            <span>STATE</span>
          </div>
          {entries.slice(0, 100).map((e, i) => (
            <div className="datarow" key={i} style={{ gridTemplateColumns: '1.4fr 1.4fr 0.6fr 0.8fr' }}>
              <span className="truncate">{e.src || '—'}</span>
              <span className="truncate">{e.dst || '—'}</span>
              <span>{e.proto || '—'}</span>
              <span>{e.state || '—'}</span>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
