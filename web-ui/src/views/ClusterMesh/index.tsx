// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { Link2, Loader2, Wifi, Clock } from 'lucide-react';
import { fetchMeshPeers, MeshPeer } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const STATUS_BADGE: Record<string, string> = { connected: 'bg-green-500/15 text-green-400 border-green-500/30', disconnected: 'bg-red-500/15 text-red-400 border-red-500/30', connecting: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30' };

const ClusterMesh: React.FC = () => {
  usePageTitle('Cluster Mesh');
  const [peers, setPeers] = useState<MeshPeer[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [success] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setPeers((await fetchMeshPeers()).data.peers ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const totalSynced = peers.reduce((a, p) => a + p.synced_endpoints, 0);

  const tick = useChanged(peers);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20"><Link2 className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Cluster Mesh</h1></div>
          <p className="text-sm text-slate-400 mt-1">ClusterMesh peer connections and sync status</p>
        </div>
        <div className="flex items-center gap-3">
          <ExportButton data={peers as unknown as Record<string, unknown>[]} filename="cluster-mesh" />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (peers.length ? `${peers.length} ClusterMesh peer${peers.length === 1 ? '' : 's'}.` : 'No ClusterMesh peers.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'Peers', value: tick ? peers.length : undefined },
          { label: 'Connected', value: tick ? peers.filter((p) => p.status === 'connected').length : undefined },
          { label: 'Synced Endpoints', value: tick ? totalSynced.toLocaleString() : undefined },
          { label: 'Synced Services', value: tick ? peers.reduce((a, p) => a + p.synced_services, 0) : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}


      {loading && peers.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {!loading && peers.length === 0 && !error && (
        <div className="text-center py-12 text-slate-400">No cluster mesh peers found.</div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {peers.map((p) => (
          <div key={p.name} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-2">
                <Wifi className={`w-5 h-5 ${p.status === 'connected' ? 'text-green-400' : 'text-red-400'}`} />
                <div><div className="font-semibold text-white">{p.name}</div><div className="text-xs text-slate-400 font-mono">{p.endpoint}</div></div>
              </div>
              <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[p.status] ?? ''}`}>{p.status}</span>
            </div>
            <div className="grid grid-cols-2 gap-3 text-sm mb-3">
              <div><span className="text-slate-400">Identities: </span><span className="font-medium text-white">{p.synced_identities}</span></div>
              <div><span className="text-slate-400">Endpoints: </span><span className="font-medium text-white">{(p.synced_endpoints ?? 0).toLocaleString()}</span></div>
              <div><span className="text-slate-400">Services: </span><span className="font-medium text-white">{p.synced_services}</span></div>
              <div><span className="text-slate-400">Latency: </span><span className={`font-medium ${p.latency_ms ?? p.latency < 50 ? 'text-green-400' : 'text-yellow-400'}`}>{p.latency_ms ?? p.latency} ms</span></div>
            </div>
            <div className="text-xs text-slate-400 flex items-center gap-1"><Clock className="w-3 h-3" /> Connected since {new Date(p.connected_since).toLocaleDateString()}</div>
          </div>
        ))}
      </div>
    </div>
  );
};

export default ClusterMesh;
