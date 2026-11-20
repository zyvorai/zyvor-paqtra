// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useCallback } from 'react';
import { Unplug, Loader2, CheckCircle, XCircle } from 'lucide-react';
import { fetchKPRStatus, KPRStatus } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

function BoolBadge({ value, label }: { value: boolean; label: string }) {
  return (
    <div className="flex items-center gap-2 p-3 rounded-lg bg-slate-900/50">
      {value ? <CheckCircle className="w-4 h-4 text-green-400" /> : <XCircle className="w-4 h-4 text-slate-400" />}
      <span className={value ? 'text-white' : 'text-slate-400'}>{label}</span>
    </div>
  );
}

const KubeProxyReplacement: React.FC = () => {
  usePageTitle('KubeProxy Replacement');
  const [kpr, setKpr] = useState<KPRStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setKpr((await fetchKPRStatus()).data); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const tick = useChanged(kpr);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-slate-500 to-slate-700 flex items-center justify-center shadow-lg shadow-slate-500/20"><Unplug className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">KubeProxy Replacement</h1></div>
          <p className="text-sm text-slate-400 mt-1">Cilium eBPF-based kube-proxy replacement status</p>
        </div>
        <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (kpr ? `kube-proxy replacement ${kpr.enabled ? 'on' : 'off'}${kpr.mode ? ` (${kpr.mode})` : ''}.` : undefined) : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'replacement', value: tick ? (kpr ? (kpr.enabled ? 'on' : 'off') : '—') : undefined, tone: tick && kpr ? (kpr.enabled ? 'ok' : 'warn') : undefined },
          { label: 'services', value: tick ? kpr?.services ?? "—" : undefined },
          { label: 'backends', value: tick ? kpr?.backends ?? "—" : undefined },
          { label: 'NAT entries', value: tick ? kpr?.nat_entries?.toLocaleString() ?? "—" : undefined },
        ]}
      />
      {loading && !kpr && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {!loading && !kpr && !error && (
        <div className="text-center py-12 text-slate-400">No KubeProxy replacement data available.</div>
      )}

      {kpr && (
        <>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4 mb-6">
            <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
              <h3 className="text-sm font-semibold text-white mb-3">Configuration</h3>
              <div className="space-y-2 text-sm">
                <div className="flex justify-between"><span className="text-slate-400">Device</span><span className="font-mono text-white">{kpr.device}</span></div>
                <div className="flex justify-between"><span className="text-slate-400">DSR Mode</span><span className="text-white">{kpr.dsr_mode}</span></div>
                <div className="flex justify-between"><span className="text-slate-400">NodePort Range</span><span className="font-mono text-white">{kpr.node_port_range}</span></div>
              </div>
            </div>

            <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
              <h3 className="text-sm font-semibold text-white mb-3">eBPF Tables</h3>
              <div className="space-y-2 text-sm">
                <div className="flex justify-between"><span className="text-slate-400">NAT Entries</span><span className="text-white">{(kpr.nat_entries ?? 0).toLocaleString()}</span></div>
                <div className="flex justify-between"><span className="text-slate-400">CT Entries</span><span className="text-white">{(kpr.ct_entries ?? 0).toLocaleString()}</span></div>
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2 mb-3"><div className="w-1 h-5 bg-gradient-to-b from-blue-400 to-cyan-500 rounded-full" /><h2 className="text-lg font-semibold text-white">Features</h2></div>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-2">
            <BoolBadge value={!!kpr.session_affinity} label="Session Affinity" />
            <BoolBadge value={!!kpr.graceful_termination} label="Graceful Termination" />
            <BoolBadge value={kpr.enabled} label="eBPF NodePort" />
            <BoolBadge value={kpr.dsr_mode !== 'disabled'} label="DSR Mode" />
          </div>
        </>
      )}
    </div>
  );
};

export default KubeProxyReplacement;
