// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { Cable, Loader2, Search, ArrowUp, ArrowDown } from 'lucide-react';
import { fetchNetInterfaces, NetInterface } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

function fmt(b: number): string { return b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : b >= 1e6 ? `${(b / 1e6).toFixed(1)} MB` : `${(b / 1e3).toFixed(1)} KB`; }
function fmtPkts(n: number): string { return n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}K` : String(n); }

const TYPE_BADGE: Record<string, string> = { physical: 'bg-blue-500/15 text-blue-400 border-blue-500/30', veth: 'bg-green-500/15 text-green-400 border-green-500/30', bridge: 'bg-purple-500/15 text-purple-400 border-purple-500/30', tunnel: 'bg-orange-500/15 text-orange-400 border-orange-500/30' };

const NetworkIfaces: React.FC = () => {
  usePageTitle('Network Interfaces');
  const [ifaces, setIfaces] = useState<NetInterface[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setIfaces((await fetchNetInterfaces()).data.interfaces ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const filtered = search ? ifaces.filter((i) => (i.name ?? '').includes(search) || (i.node ?? '').includes(search) || (i.ipv4 ?? (Array.isArray(i.addresses) ? i.addresses[0] : i.address ?? '')).includes(search)) : ifaces;

  const tick = useChanged(ifaces);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-teal-500 to-teal-700 flex items-center justify-center shadow-lg shadow-teal-500/20"><Cable className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Network Interfaces</h1></div>
          <p className="text-sm text-slate-400 mt-1">Interface-level monitoring with traffic stats</p>
        </div>
        <div className="flex items-center gap-3">
          <ExportButton data={filtered as unknown as Record<string, unknown>[]} filename="network-interfaces" />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? `${ifaces.length} interfaces, ${ifaces.filter((i) => /up/i.test(i.state)).length} up.` : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'interfaces', value: tick ? ifaces.length : undefined },
          { label: 'up', value: tick ? ifaces.filter((i) => /up/i.test(i.state)).length : undefined },
          { label: 'rx errors', value: tick ? ifaces.reduce((n, i) => n + (i.rx_errors || 0), 0) : undefined, tone: tick ? countTone(Number(ifaces.reduce((n, i) => n + (i.rx_errors || 0), 0))) : undefined },
          { label: 'nodes', value: tick ? new Set(ifaces.map((i) => i.node)).size : undefined },
        ]}
      />

      <div className="relative mb-4"><Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400" /><input type="text" value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search interfaces, nodes, IPs..." className="w-full pl-9 pr-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500" /></div>

      {loading && ifaces.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {!loading && filtered.length === 0 && !error && (
        <div className="text-center py-12 text-slate-400">No network interfaces found.</div>
      )}

      <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead><tr className="border-b border-slate-700/50 bg-slate-900/50">
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Interface</th>
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Node</th>
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Type</th>
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">IPv4</th>
              <th className="text-right px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">MTU</th>
              <th className="text-right px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">RX</th>
              <th className="text-right px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">TX</th>
              <th className="text-right px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">RX Pkts</th>
              <th className="text-right px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Errors</th>
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">State</th>
            </tr></thead>
            <tbody>{filtered.map((i) => (
              <tr key={`${i.node}-${i.name}`} className="border-b border-slate-700/30 table-row-hover">
                <td className="px-4 py-2.5 font-mono text-white">{i.name}</td>
                <td className="px-4 py-2.5 text-slate-400">{i.node}</td>
                <td className="px-4 py-2.5"><span className={`px-2 py-0.5 rounded-full text-xs border ${TYPE_BADGE[i.type] ?? 'bg-slate-900/50 text-slate-400 border-slate-700/50'}`}>{i.type}</span></td>
                <td className="px-4 py-2.5 font-mono text-white">{i.ipv4 ?? (Array.isArray(i.addresses) ? i.addresses[0] : i.address ?? '-')}</td>
                <td className="px-4 py-2.5 text-right text-slate-400">{i.mtu}</td>
                <td className="px-4 py-2.5 text-right"><span className="flex items-center justify-end gap-1 text-white"><ArrowDown className="w-3 h-3 text-green-400" />{fmt(i.rx_bytes ?? i.stats?.rx_bytes ?? 0)}</span></td>
                <td className="px-4 py-2.5 text-right"><span className="flex items-center justify-end gap-1 text-white"><ArrowUp className="w-3 h-3 text-blue-400" />{fmt(i.tx_bytes ?? i.stats?.tx_bytes ?? 0)}</span></td>
                <td className="px-4 py-2.5 text-right text-slate-400">{fmtPkts(i.rx_packets ?? i.stats?.rx_packets ?? 0)}</td>
                <td className="px-4 py-2.5 text-right"><span className={(i.rx_errors ?? i.stats?.rx_errors ?? 0) + (i.tx_errors ?? i.stats?.tx_errors ?? 0) > 0 ? 'text-red-400' : 'text-green-400'}>{(i.rx_errors ?? i.stats?.rx_errors ?? 0) + (i.tx_errors ?? i.stats?.tx_errors ?? 0)}</span></td>
                <td className="px-4 py-2.5"><span className={`w-2 h-2 rounded-full inline-block ${i.state === 'up' ? 'bg-green-400' : 'bg-red-400'}`} /> <span className="text-slate-400 ml-1">{i.state}</span></td>
              </tr>
            ))}</tbody>
          </table>
        </div>
      </div>
    </div>
  );
};

export default NetworkIfaces;
