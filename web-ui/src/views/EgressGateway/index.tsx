// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useCallback } from 'react';
import { LogOut, Loader2 } from 'lucide-react';
import { fetchEgressPolicies, EgressPolicy } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const STATUS_BADGE: Record<string, string> = { active: 'bg-green-500/15 text-green-400 border-green-500/30', pending: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30', error: 'bg-red-500/15 text-red-400 border-red-500/30' };

const EgressGateway: React.FC = () => {
  usePageTitle('Egress Gateway');
  const [policies, setPolicies] = useState<EgressPolicy[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setPolicies((await fetchEgressPolicies()).data.policies ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const tick = useChanged(policies);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20"><LogOut className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Egress Gateway</h1></div>
          <p className="text-sm text-slate-400 mt-1">Egress IP masquerading and gateway node policies</p>
        </div>
        <div className="flex items-center gap-3">
          <ExportButton data={policies as unknown as Record<string, unknown>[]} filename="egress-policies" />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (policies.length ? `${policies.length} egress gateway polic${policies.length === 1 ? 'y' : 'ies'}.` : 'No egress gateway policies.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'policies', value: tick ? policies.length : undefined },
          { label: 'gateway nodes', value: tick ? new Set(policies.map((p) => p.gateway_node)).size : undefined },
          { label: 'destination CIDRs', value: tick ? policies.reduce((n, p) => n + (p.destination_cidrs?.length ?? 0), 0) : undefined },
        ]}
      />
      {loading && policies.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {policies.map((p) => (
          <div key={p.name} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
            <div className="flex items-center justify-between mb-3">
              <span className="font-semibold text-white">{p.namespace}/{p.name}</span>
              <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[p.status] ?? ''}`}>{p.status}</span>
            </div>
            <div className="space-y-2 text-sm">
              <div className="flex justify-between"><span className="text-slate-400">Gateway Node</span><span className="text-white">{p.gateway_node}</span></div>
              <div className="flex justify-between"><span className="text-slate-400">Egress IP</span><span className="font-mono text-white">{p.egress_ip}</span></div>
              <div>
                <span className="text-slate-400 text-xs">Destination CIDRs</span>
                <div className="flex flex-wrap gap-1 mt-1">{p.destination_cidrs.map((c) => <span key={c} className="px-2 py-0.5 rounded bg-slate-900/50 text-xs font-mono">{c}</span>)}</div>
              </div>
              <div>
                <span className="text-slate-400 text-xs">Pod Selectors</span>
                <div className="flex flex-wrap gap-1 mt-1">{(Array.isArray(p.selectors) ? p.selectors : [p.selectors]).map((s: string) => <span key={s} className="px-2 py-0.5 rounded bg-slate-900/50 text-xs">{s}</span>)}</div>
              </div>
            </div>
          </div>
        ))}
        {!loading && policies.length === 0 && !error && <div className="col-span-full text-center py-12 text-slate-400">No egress gateway policies configured.</div>}
      </div>
    </div>
  );
};

export default EgressGateway;
