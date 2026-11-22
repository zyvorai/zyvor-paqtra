// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { Shield, Loader2, ArrowUpDown, Clock } from 'lucide-react';
import { fetchWireGuardPeers, WireGuardPeer } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

function fmt(b: number): string { return b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : b >= 1e6 ? `${(b / 1e6).toFixed(1)} MB` : `${(b / 1e3).toFixed(1)} KB`; }

const WireGuardPeers: React.FC = () => {
  usePageTitle('WireGuard Peers');
  const [peers, setPeers] = useState<WireGuardPeer[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setPeers((await fetchWireGuardPeers()).data.peers ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const tick = useChanged(peers);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-red-500 to-red-700 flex items-center justify-center shadow-lg shadow-red-500/20"><Shield className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">WireGuard Peers</h1></div>
          <p className="text-sm text-slate-400 mt-1">Node-to-node WireGuard tunnel status</p>
        </div>
        <div className="flex items-center gap-3">
          <ExportButton data={peers as unknown as Record<string, unknown>[]} filename="wireguard-peers" />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (peers.length ? `${peers.length} WireGuard peer${peers.length === 1 ? '' : 's'} across ${new Set(peers.map((p) => p.node)).size} nodes.` : 'No WireGuard peers.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'peers', value: tick ? peers.length : undefined },
          { label: 'nodes', value: tick ? new Set(peers.map((p) => p.node)).size : undefined },
          { label: 'allowed IPs', value: tick ? peers.reduce((n, p) => n + (p.allowed_ips?.length ?? 0), 0) : undefined },
        ]}
      />
      {loading && peers.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {!loading && peers.length === 0 && !error && (
        <div className="text-center py-12 text-slate-400">No WireGuard peers found.</div>
      )}

      <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
        <table className="w-full text-sm">
          <thead><tr className="border-b border-slate-700/50 bg-slate-900/50">
            <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Node</th>
            <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Public Key</th>
            <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Endpoint</th>
            <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Allowed IPs</th>
            <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Handshake</th>
            <th className="text-right px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">RX / TX</th>
          </tr></thead>
          <tbody>{peers.map((p) => (
            <tr key={p.public_key} className="border-b border-slate-700/30 table-row-hover">
              <td className="px-4 py-2.5 font-medium text-white">{p.node}</td>
              <td className="px-4 py-2.5 font-mono text-xs text-slate-400 truncate max-w-[200px]" title={p.public_key}>{p.public_key.slice(0, 20)}...</td>
              <td className="px-4 py-2.5 font-mono text-white">{p.endpoint}</td>
              <td className="px-4 py-2.5">{p.allowed_ips.map((ip) => <span key={ip} className="inline-block px-1.5 py-0.5 rounded bg-slate-900/50 text-xs mr-1 mb-0.5">{ip}</span>)}</td>
              <td className="px-4 py-2.5 text-slate-400 flex items-center gap-1"><Clock className="w-3 h-3" />{p.latest_handshake}</td>
              <td className="px-4 py-2.5 text-right"><span className="flex items-center justify-end gap-1"><ArrowUpDown className="w-3 h-3 text-slate-400" />{fmt(p.transfer_rx)} / {fmt(p.transfer_tx)}</span></td>
            </tr>
          ))}</tbody>
        </table>
      </div>
    </div>
  );
};

export default WireGuardPeers;
