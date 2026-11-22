// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback, useMemo } from 'react';
import { Globe } from 'lucide-react';
import { fetchEbpfIpcache } from '../../services/api';
import { isAxiosError } from 'axios';
import { formatCount } from '../../utils/formatters';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import Pagination from '../../components/Pagination';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

interface IPCacheEntry {
  ip: string;
  cidr: string;
  identity: number;
  [key: string]: unknown;
}

const IDENTITY_COLORS = [
  'bg-blue-500/20 text-blue-400 border-blue-500/30',
  'bg-green-500/20 text-green-400 border-green-500/30',
  'bg-purple-500/20 text-purple-400 border-purple-500/30',
  'bg-orange-500/20 text-orange-400 border-orange-500/30',
  'bg-cyan-500/20 text-cyan-400 border-cyan-500/30',
  'bg-pink-500/20 text-pink-400 border-pink-500/30',
  'bg-yellow-500/20 text-yellow-400 border-yellow-500/30',
];

function identityColor(id: number): string {
  return IDENTITY_COLORS[id % IDENTITY_COLORS.length];
}

const IPCacheViewer: React.FC = () => {
  usePageTitle('IP Cache');
  const [entries, setEntries] = useState<IPCacheEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);
  const [search, setSearch] = useState('');
  const [currentPage, setCurrentPage] = useState(1);
  const PAGE_SIZE = 100;

  const loadData = useCallback(async () => {
    setError(null);
    try {
      const res = await fetchEbpfIpcache();
      setEntries((res.data.entries ?? []) as IPCacheEntry[]);
    } catch (err) {
      setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed to load IP cache data');
    }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(loadData, 30000, autoRefreshOn);

  const filtered = useMemo(() => {
    if (!search) return entries;
    const q = search.toLowerCase();
    return entries.filter((e) => (e.ip ?? e.cidr ?? '').toLowerCase().includes(q));
  }, [entries, search]);

  const paginatedFiltered = useMemo(() => {
    const start = (currentPage - 1) * PAGE_SIZE;
    return filtered.slice(start, start + PAGE_SIZE);
  }, [filtered, currentPage]);

  const tick = useChanged(entries);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-emerald-500 to-emerald-700 flex items-center justify-center shadow-lg shadow-emerald-500/20">
              <Globe className="w-5 h-5 text-white" />
            </div>
            <h1 className="text-2xl font-bold text-white">IP Cache</h1>
          </div>
          <p className="text-sm text-slate-400 mt-1">IP to identity mapping from kernel</p>
        </div>
        <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
      </div>

      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? `${entries.length.toLocaleString()} IP cache entries, read-only.` : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'Total Entries', value: tick ? formatCount(entries.length) : undefined },
          { label: 'Unique Identities', value: tick ? formatCount(new Set(entries.map((e) => e.identity)).size) : undefined },
        ]}
      />


      <div className="mb-4">
        <input
          type="text"
          placeholder="Search by IP or CIDR..."
          value={search}
          onChange={(e) => { setSearch(e.target.value); setCurrentPage(1); }}
          className="w-full max-w-sm px-4 py-2 rounded-lg bg-slate-800 border border-slate-700 text-white text-sm placeholder-slate-500 focus:outline-none focus:border-emerald-500"
        />
      </div>

      <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
        {filtered.length > 0 && (
          <div className="px-4 py-3 border-b border-slate-700/50 flex justify-end">
            <ExportButton data={filtered as unknown as Record<string, unknown>[]} filename="ipcache-entries" />
          </div>
        )}
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="bg-slate-900/50">
              <tr className="text-left text-slate-400">
                <th className="px-4 py-3 font-medium">IP / CIDR</th>
                <th className="px-4 py-3 font-medium">Identity</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-700/50">
              {paginatedFiltered.map((e, i) => (
                <tr key={i} className="table-row-hover">
                  <td className="px-4 py-3 text-white font-mono text-xs">{e.ip || e.cidr}</td>
                  <td className="px-4 py-3">
                    <span className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium border ${identityColor(e.identity)}`}>
                      {e.identity}
                    </span>
                  </td>
                </tr>
              ))}
              {filtered.length === 0 && !loading && (
                <tr><td colSpan={2} className="px-4 py-8 text-center text-slate-400">No IP cache entries found</td></tr>
              )}
            </tbody>
          </table>
        </div>
        <Pagination
          currentPage={currentPage}
          totalItems={filtered.length}
          pageSize={PAGE_SIZE}
          onPageChange={setCurrentPage}
        />
      </div>
    </div>
  );
};

export default IPCacheViewer;
