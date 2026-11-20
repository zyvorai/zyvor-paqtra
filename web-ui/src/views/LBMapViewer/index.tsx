// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useCallback, useMemo } from 'react';
import { Scale } from 'lucide-react';
import { fetchEbpfLb } from '../../services/api';
import { isAxiosError } from 'axios';
import { formatCount } from '../../utils/formatters';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import Pagination from '../../components/Pagination';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

interface LBEntry {
  service_ip: string;
  service_port: number;
  backend_slot: number;
  protocol: number;
  backend_ip?: string;
  backend_port?: number;
  [key: string]: unknown;
}

const PROTO_MAP: Record<number, string> = { 1: 'ICMP', 6: 'TCP', 17: 'UDP' };

const LBMapViewer: React.FC = () => {
  usePageTitle('LB Map');
  const [entries, setEntries] = useState<LBEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);
  const [currentPage, setCurrentPage] = useState(1);
  const PAGE_SIZE = 100;

  const loadData = useCallback(async () => {
    setError(null);
    try {
      const res = await fetchEbpfLb();
      setEntries((res.data.entries ?? []) as unknown as LBEntry[]);
    } catch (err) {
      setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed to load LB map data');
    }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(loadData, 30000, autoRefreshOn);

  const paginatedEntries = useMemo(() => {
    const start = (currentPage - 1) * PAGE_SIZE;
    return entries.slice(start, start + PAGE_SIZE);
  }, [entries, currentPage]);

  const tick = useChanged(entries);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-violet-500 to-violet-700 flex items-center justify-center shadow-lg shadow-violet-500/20">
              <Scale className="w-5 h-5 text-white" />
            </div>
            <h1 className="text-2xl font-bold text-white">Load Balancer Map</h1>
          </div>
          <p className="text-sm text-slate-400 mt-1">Service to backend mappings from kernel</p>
        </div>
        <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
      </div>

      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? `${entries.length.toLocaleString()} Cilium LB map entries, read-only.` : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'Total Entries', value: tick ? formatCount(entries.length) : undefined },
          { label: 'Unique Services', value: tick ? formatCount(new Set(entries.map((e) => `${e.service_ip}:${e.service_port}`)).size) : undefined },
        ]}
      />


      <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
        {entries.length > 0 && (
          <div className="px-4 py-3 border-b border-slate-700/50 flex justify-end">
            <ExportButton data={entries as unknown as Record<string, unknown>[]} filename="lb-map" />
          </div>
        )}
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="bg-slate-900/50">
              <tr className="text-left text-slate-400">
                <th className="px-4 py-3 font-medium">Service IP</th>
                <th className="px-4 py-3 font-medium">Service Port</th>
                <th className="px-4 py-3 font-medium">Backend Slot</th>
                <th className="px-4 py-3 font-medium">Protocol</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-700/50">
              {paginatedEntries.map((e, i) => (
                <tr key={i} className="table-row-hover">
                  <td className="px-4 py-3 text-white font-mono text-xs">{e.service_ip}</td>
                  <td className="px-4 py-3 text-slate-300">{e.service_port}</td>
                  <td className="px-4 py-3 text-slate-300">{e.backend_slot}</td>
                  <td className="px-4 py-3">
                    <span className={`px-2 py-0.5 rounded text-xs font-medium ${
                      e.protocol === 6 ? 'bg-green-500/20 text-green-400' :
                      e.protocol === 17 ? 'bg-purple-500/20 text-purple-400' :
                      'bg-slate-500/20 text-slate-400'
                    }`}>{PROTO_MAP[e.protocol] ?? `Proto ${e.protocol}`}</span>
                  </td>
                </tr>
              ))}
              {entries.length === 0 && !loading && (
                <tr><td colSpan={4} className="px-4 py-8 text-center text-slate-400">No LB map entries found</td></tr>
              )}
            </tbody>
          </table>
        </div>
        <Pagination
          currentPage={currentPage}
          totalItems={entries.length}
          pageSize={PAGE_SIZE}
          onPageChange={setCurrentPage}
        />
      </div>
    </div>
  );
};

export default LBMapViewer;
