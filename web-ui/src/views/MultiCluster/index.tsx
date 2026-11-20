// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useCallback } from 'react';
import { Globe, Loader2, Wifi, WifiOff, ArrowRightLeft, Clock } from 'lucide-react';
import { fetchClusters, syncClusterPolicies, ClusterInfo, apiErrorMessage } from '../../services/api';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const STATUS_BADGE: Record<string, string> = {
  connected: 'bg-green-500/15 text-green-400 border-green-500/30',
  degraded: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30',
  disconnected: 'bg-red-500/15 text-red-400 border-red-500/30',
};

/** Flip to true once the backend action is implemented. */
const SYNC_AVAILABLE = false;
const NOT_AVAILABLE_MSG = 'Cross-cluster policy sync is not implemented yet: apply policies to each cluster directly for now.';

const MultiCluster: React.FC = () => {
  usePageTitle('Multi-Cluster');
  const [clusters, setClusters] = useState<ClusterInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [syncing, setSyncing] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setClusters((await fetchClusters()).data.clusters ?? []); }
    catch (err) { setError(apiErrorMessage(err, 'Failed')); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const handleSync = async (name: string) => {
    setSyncing(name); setError(null);
    try { await syncClusterPolicies(name); setSuccess(`Sync initiated for ${name}`); }
    catch (err) { setError(apiErrorMessage(err, 'Sync failed')); }
    finally { setSyncing(null); }
  };

  const totalNodes = clusters.reduce((a, c) => a + c.nodes, 0);
  const totalPods = clusters.reduce((a, c) => a + c.pods, 0);

  const tick = useChanged(clusters);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20"><Globe className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Multi-Cluster</h1></div>
          <p className="text-sm text-slate-400 mt-1">Cross-cluster topology, health checks, and policy sync</p>
        </div>
        <div className="flex items-center gap-3">
          {clusters.length > 0 && <ExportButton data={clusters as unknown as Record<string, unknown>[]} filename="clusters" />}
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>

      {!SYNC_AVAILABLE && <div className="mb-4 p-3 rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-300 text-sm">{NOT_AVAILABLE_MSG}</div>}
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (clusters.length ? `${clusters.length} cluster${clusters.length === 1 ? '' : 's'} in view.` : 'No other clusters registered.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'Clusters', value: tick ? clusters.length : undefined },
          { label: 'Connected', value: tick ? clusters.filter((c) => c.status === 'connected').length : undefined },
          { label: 'Total Nodes', value: tick ? totalNodes : undefined },
          { label: 'Total Pods', value: tick ? totalPods : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      {/* Summary */}

      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {/* Cluster cards */}
      {!loading && clusters.length === 0 && (
        <div className="text-center py-12 text-slate-400">
          <Globe className="w-12 h-12 mx-auto mb-3 text-blue-400" />
          <div className="font-medium text-white">No clusters found</div>
          <div className="text-sm">No remote clusters are currently configured.</div>
        </div>
      )}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        {clusters.map((c) => (
          <div key={c.name} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-2">
                {c.status === 'connected' ? <Wifi className="w-5 h-5 text-green-400" /> : <WifiOff className="w-5 h-5 text-red-400" />}
                <div>
                  <div className="font-semibold text-white">{c.name}</div>
                  <div className="text-xs text-slate-400">{c.region}</div>
                </div>
              </div>
              <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[c.status] ?? ''}`}>{c.status}</span>
            </div>

            <div className="grid grid-cols-2 gap-3 text-sm mb-4">
              <div><span className="text-slate-400">Nodes: </span><span className="font-medium text-white">{c.nodes}</span></div>
              <div><span className="text-slate-400">Pods: </span><span className="font-medium text-white">{c.pods}</span></div>
              <div><span className="text-slate-400">Latency: </span><span className={`font-medium ${c.latency_ms ?? c.latency < 50 ? 'text-green-400' : c.latency_ms ?? c.latency < 100 ? 'text-yellow-400' : 'text-red-400'}`}>{c.latency_ms ?? c.latency} ms</span></div>
              <div><span className="text-slate-400">Cilium: </span><span className="font-mono text-white">{c.cilium_version}</span></div>
            </div>

            <div className="flex items-center gap-2 text-xs text-slate-400 mb-4">
              <Clock className="w-3.5 h-3.5" />
              Last sync: {new Date(c.last_sync).toLocaleTimeString()}
            </div>

            <button onClick={() => handleSync(c.name)} disabled={syncing === c.name || !SYNC_AVAILABLE} title={SYNC_AVAILABLE ? undefined : NOT_AVAILABLE_MSG}
              className="w-full flex items-center justify-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm hover:bg-slate-700/30 disabled:opacity-50 transition-colors">
              {syncing === c.name ? <Loader2 className="w-4 h-4 animate-spin" /> : <ArrowRightLeft className="w-4 h-4" />}
              Sync Policies
            </button>
          </div>
        ))}
      </div>
    </div>
  );
};

export default MultiCluster;
