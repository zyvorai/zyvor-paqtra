// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useCallback } from 'react';
import { Shield } from 'lucide-react';
import api from '../../services/api';
import { isAxiosError } from 'axios';
import { formatCount, formatBytes } from '../../utils/formatters';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

interface PolicyMapEntry {
  src_identity: number;
  dst_port: number;
  protocol: number;
  packets: number;
  bytes: number;
  [key: string]: unknown;
}

const PROTO_MAP: Record<number, string> = { 1: 'ICMP', 6: 'TCP', 17: 'UDP' };

const PolicyMapViewer: React.FC = () => {
  usePageTitle('Policy Map');
  const [entries, setEntries] = useState<PolicyMapEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const loadData = useCallback(async () => {
    setError(null);
    try {
      const res = await api.get<{ entries: unknown[]; total: number }>('/ebpf/policy-map');
      setEntries((res.data.entries ?? []) as PolicyMapEntry[]);
    } catch (err) {
      setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed to load policy map data');
    }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(loadData, 30000, autoRefreshOn);

  const tick = useChanged(entries);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20">
              <Shield className="w-5 h-5 text-white" />
            </div>
            <h1 className="text-2xl font-bold text-white">Policy Map</h1>
          </div>
          <p className="text-sm text-slate-400 mt-1">Kernel-level policy enforcement decisions</p>
        </div>
        <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
      </div>

      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? `${entries.length.toLocaleString()} policy map entries, read-only.` : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'entries', value: tick ? entries.length : undefined },
          { label: 'identities', value: tick ? new Set(entries.map((e) => e.src_identity)).size : undefined },
          { label: 'packets', value: tick ? entries.reduce((n, e) => n + (e.packets || 0), 0).toLocaleString() : undefined },
        ]}
      />

      <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
        {entries.length > 0 && (
          <div className="px-4 py-3 border-b border-slate-700/50 flex justify-end">
            <ExportButton data={entries as unknown as Record<string, unknown>[]} filename="policy-map" />
          </div>
        )}
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead className="bg-slate-900/50">
              <tr className="text-left text-slate-400">
                <th className="px-4 py-3 font-medium">Source Identity</th>
                <th className="px-4 py-3 font-medium">Dest Port</th>
                <th className="px-4 py-3 font-medium">Protocol</th>
                <th className="px-4 py-3 font-medium text-right">Packets</th>
                <th className="px-4 py-3 font-medium text-right">Bytes</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-700/50">
              {entries.map((e, i) => (
                <tr key={i} className="table-row-hover">
                  <td className="px-4 py-3">
                    <span className="inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium bg-blue-500/20 text-blue-400 border border-blue-500/30">
                      {e.src_identity}
                    </span>
                  </td>
                  <td className="px-4 py-3 text-slate-300">{e.dst_port}</td>
                  <td className="px-4 py-3">
                    <span className={`px-2 py-0.5 rounded text-xs font-medium ${
                      e.protocol === 6 ? 'bg-green-500/20 text-green-400' :
                      e.protocol === 17 ? 'bg-purple-500/20 text-purple-400' :
                      'bg-slate-500/20 text-slate-400'
                    }`}>{PROTO_MAP[e.protocol] ?? `Proto ${e.protocol}`}</span>
                  </td>
                  <td className="px-4 py-3 text-right text-slate-300">{formatCount(e.packets)}</td>
                  <td className="px-4 py-3 text-right text-slate-300">{formatBytes(e.bytes)}</td>
                </tr>
              ))}
              {entries.length === 0 && !loading && (
                <tr><td colSpan={5} className="px-4 py-8 text-center text-slate-400">No policy map entries found</td></tr>
              )}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
};

export default PolicyMapViewer;
