// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { Network, Loader2, Search } from 'lucide-react';
import { fetchIPAMPools, fetchIPAllocations, IPAMPool, IPAllocation } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const IPAM: React.FC = () => {
  usePageTitle('IPAM');
  const [pools, setPools] = useState<IPAMPool[]>([]);
  const [allocations, setAllocations] = useState<IPAllocation[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<'pools' | 'allocations'>('pools');
  const [search, setSearch] = useState('');
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { const [p, a] = await Promise.all([fetchIPAMPools(), fetchIPAllocations()]); setPools(p.data.pools ?? []); setAllocations(a.data.allocations ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const totalAllocated = pools.reduce((a, p) => a + p.allocated, 0);
  const totalAvailable = pools.reduce((a, p) => a + p.available, 0);
  const filteredAllocs = search ? allocations.filter((a) => a.ip.includes(search) || a.pod.includes(search) || a.namespace.includes(search)) : allocations;

  const tick = useChanged(pools);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20"><Network className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">IP Address Management</h1></div>
          <p className="text-sm text-slate-400 mt-1">IPAM pool usage and IP allocation tracking</p>
        </div>
        <div className="flex items-center gap-3">
          <ExportButton data={tab === 'pools' ? pools as unknown as Record<string, unknown>[] : filteredAllocs as unknown as Record<string, unknown>[]} filename={tab === 'pools' ? 'ipam-pools' : 'ipam-allocations'} />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? `${pools.length} IP pool${pools.length === 1 ? '' : 's'}, ${totalAllocated} addresses allocated.` : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'Pools', value: tick ? pools.length : undefined },
          { label: 'Allocated IPs', value: tick ? totalAllocated : undefined },
          { label: 'Available IPs', value: tick ? totalAvailable : undefined },
        ]}
      />


      <div className="flex gap-1 mb-4 p-1 rounded-lg bg-slate-900/50 w-fit">
        <button onClick={() => setTab('pools')} className={`px-4 py-2 rounded-md text-sm transition-colors ${tab === 'pools' ? 'bg-slate-800/50 text-white shadow' : 'text-slate-400'}`}>Pools ({pools.length})</button>
        <button onClick={() => setTab('allocations')} className={`px-4 py-2 rounded-md text-sm transition-colors ${tab === 'allocations' ? 'bg-slate-800/50 text-white shadow' : 'text-slate-400'}`}>Allocations ({allocations.length})</button>
      </div>

      {loading && pools.length === 0 && allocations.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {tab === 'pools' && (
        <>
          {!loading && pools.length === 0 && !error && (
            <div className="text-center py-12 text-slate-400">No IPAM pools found.</div>
          )}
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {pools.map((p) => {
              const usagePct = p.usage_pct ?? (typeof p.utilization === 'string' ? parseFloat(p.utilization) : p.total > 0 ? (p.allocated / p.total) * 100 : 0);
              const color = usagePct >= 90 ? 'bg-red-400' : usagePct >= 70 ? 'bg-yellow-400' : 'bg-green-400';
              return (
                <div key={p.name} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
                  <div className="font-semibold text-white mb-1">{p.name}</div>
                  <div className="text-sm text-slate-400 font-mono mb-3">{p.cidr}</div>
                  <div className="flex justify-between text-sm mb-1">
                    <span className="text-slate-400">Usage</span>
                    <span className="font-medium text-white">{p.allocated} / {p.total} ({Math.round(usagePct)}%)</span>
                  </div>
                  <div className="w-full h-2.5 rounded-full bg-slate-700 overflow-hidden">
                    <div className={`h-full rounded-full ${color}`} style={{ width: `${usagePct}%` }} />
                  </div>
                  <div className="flex justify-between text-xs text-slate-400 mt-2">
                    <span>{p.available} available</span>
                  </div>
                </div>
              );
            })}
          </div>
        </>
      )}

      {tab === 'allocations' && (
        <>
          <div className="relative mb-4"><Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400" /><input type="text" value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search IP, pod, namespace..." className="w-full pl-9 pr-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500" /></div>
          {!loading && filteredAllocs.length === 0 && !error && (
            <div className="text-center py-12 text-slate-400">No IP allocations found.</div>
          )}
          <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
            <table className="w-full text-sm">
              <thead><tr className="border-b border-slate-700/50 bg-slate-900/50">
                <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">IP</th>
                <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Pod</th>
                <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Namespace</th>
                <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Node</th>
                <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Pool</th>
              </tr></thead>
              <tbody>{filteredAllocs.map((a) => (
                <tr key={a.ip} className="border-b border-slate-700/30 table-row-hover">
                  <td className="px-4 py-2.5 font-mono text-white">{a.ip}</td>
                  <td className="px-4 py-2.5 text-white">{a.pod}</td>
                  <td className="px-4 py-2.5"><span className="px-2 py-0.5 rounded border border-slate-700/50 text-xs">{a.namespace}</span></td>
                  <td className="px-4 py-2.5 text-slate-400">{a.node}</td>
                  <td className="px-4 py-2.5 text-slate-400">{a.pool}</td>
                </tr>
              ))}</tbody>
            </table>
          </div>
        </>
      )}
    </div>
  );
};

export default IPAM;
