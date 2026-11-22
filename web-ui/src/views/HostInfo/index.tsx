// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import { fetchHostInfo, type HostInfo } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

export default function HostInfoView() {
  const [info, setInfo] = useState<HostInfo | null>(null);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      setInfo((await fetchHostInfo()).data);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const tick = useChanged(info);
  return (
    <Board>
      <PagePulse
        headline={tick ? (info ? `${info.hostname || 'host'} · ${info.kernel || 'kernel unknown'}` : undefined) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'hostname', value: tick ? info?.hostname || '—' : undefined },
          { label: 'kernel', value: tick ? info?.kernel || '—' : undefined },
          { label: 'os', value: tick ? info?.os || '—' : undefined },
          { label: 'CPU cores', value: tick ? info?.cpu_cores ?? '—' : undefined },
          { label: 'memory GB', value: tick ? info?.memory_total_gb ?? '—' : undefined },
          { label: 'arch', value: tick ? info?.arch || '—' : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>HOST</Eyebrow>
        <h3>Host and kernel</h3>
        {!info && !err ? <Empty>Waiting for host info…</Empty> : null}
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
    </Board>
  );
}
