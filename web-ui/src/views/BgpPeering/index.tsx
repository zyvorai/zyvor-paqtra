// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { Route, Loader2, Wifi, WifiOff } from 'lucide-react';
import { fetchBgpPeers, BgpPeer } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const STATE_BADGE: Record<string, string> = { Established: 'bg-green-500/15 text-green-400 border-green-500/30', Idle: 'bg-red-500/15 text-red-400 border-red-500/30', Active: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30', OpenSent: 'bg-blue-500/15 text-blue-400 border-blue-500/30' };

const BgpPeering: React.FC = () => {
  usePageTitle('BGP Peering');
  const [peers, setPeers] = useState<BgpPeer[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setPeers((await fetchBgpPeers()).data.peers ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const established = peers.filter((p) => p.state === 'Established').length;

  const tick = useChanged(peers);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-teal-500 to-teal-700 flex items-center justify-center shadow-lg shadow-teal-500/20"><Route className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">BGP Peering</h1></div>
          <p className="text-sm text-slate-400 mt-1">BGP peer status and route advertisements</p>
        </div>
        <div className="flex items-center gap-3">
          <ExportButton data={peers as unknown as Record<string, unknown>[]} filename="bgp-peers" />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (peers.length ? `${established} of ${peers.length} BGP peers established.` : 'No BGP peers configured.') : undefined}
        tone={tick ? (peers.length - established ? 'warn' : undefined) : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'Peers', value: tick ? peers.length : undefined },
          { label: 'Established', value: tick ? established : undefined },
          { label: 'Prefixes Advertised', value: tick ? peers.reduce((a, p) => a + p.prefixes_advertised, 0) : undefined },
          { label: 'not established', value: tick ? peers.length - established : undefined },
        ]}
      />


      {loading && peers.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {!loading && peers.length === 0 && !error && (
        <div className="text-center py-12 text-slate-400">No BGP peers found.</div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        {peers.map((p) => (
          <div key={p.name} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-2">
                {p.state === 'Established' ? <Wifi className="w-5 h-5 text-green-400" /> : <WifiOff className="w-5 h-5 text-red-400" />}
                <div><div className="font-semibold text-white">{p.name}</div><div className="text-xs text-slate-400 font-mono">{p.peer_address}</div></div>
              </div>
              <span className={`px-2 py-0.5 rounded-full text-xs border ${STATE_BADGE[p.state] ?? ''}`}>{p.state}</span>
            </div>
            <div className="grid grid-cols-2 gap-2 text-sm">
              <div><span className="text-slate-400">Peer ASN: </span><span className="font-mono text-white">{p.peer_asn}</span></div>
              <div><span className="text-slate-400">Local ASN: </span><span className="font-mono text-white">{p.local_asn}</span></div>
              <div><span className="text-slate-400">Uptime: </span><span className="text-white">{p.uptime}</span></div>
              <div><span className="text-slate-400">Rx Prefixes: </span><span className="text-white">{p.prefixes_received}</span></div>
              <div><span className="text-slate-400">Tx Prefixes: </span><span className="text-white">{p.prefixes_advertised}</span></div>
              <div><span className="text-slate-400">Messages: </span><span className="text-white">{(p.messages_received ?? 0).toLocaleString()} / {(p.messages_sent ?? 0).toLocaleString()}</span></div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

export default BgpPeering;
