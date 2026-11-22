// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import { fetchEncryptionStatus, fetchWireGuardPeers } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

export default function Encryption() {
  const [status, setStatus] = useState<Record<string, unknown> | null>(null);
  const [peers, setPeers] = useState<{ name?: string; status?: string; endpoint?: string }[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      const [e, w] = await Promise.allSettled([fetchEncryptionStatus(), fetchWireGuardPeers()]);
      if (e.status === 'fulfilled') setStatus(e.value.data as Record<string, unknown>);
      if (w.status === 'fulfilled') setPeers(((w.value.data as { peers?: typeof peers }).peers) ?? []);
      const failed = [e, w].find((r) => r.status === 'rejected') as PromiseRejectedResult | undefined;
      setErr(failed ? String(failed.reason) : '');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const tick = useChanged(status);
  return (
    <Board>
      <PagePulse
        headline={tick ? `Transparent encryption: ${String(status?.mode ?? status?.status ?? 'unknown')}.` : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'mode', value: tick ? String(status?.mode ?? status?.status ?? '—') : undefined },
          { label: 'WireGuard peers', value: tick ? peers.length : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>ENCRYPTION</Eyebrow>
        <h3>Encryption on the wire</h3>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
      <Card span={3}>
        <Eyebrow>WIREGUARD</Eyebrow>
        {peers.length === 0 ? <Empty>No WireGuard peers reported.</Empty> : null}
        <div className="list">
          {peers.map((p, i) => (
            <div className="agent wide" key={p.name || i}>
              <b>{p.name || `peer-${i}`}</b>
              <span>{p.status || '—'}</span>
              <small>{p.endpoint || '—'}</small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
