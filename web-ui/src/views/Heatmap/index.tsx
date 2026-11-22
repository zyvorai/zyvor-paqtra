// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { useNavigate } from 'react-router-dom';
import { Grid3X3, Loader2 } from 'lucide-react';
import { fetchHeatmapData, HeatmapCell } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import { useNamespaceStore } from '../../stores/namespaceStore';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

function cellColor(count: number, max: number): string {
  if (max === 0) return 'bg-slate-900/50';
  const ratio = count / max;
  if (ratio > 0.7) return 'bg-blue-500';
  if (ratio > 0.4) return 'bg-blue-500/60';
  if (ratio > 0.1) return 'bg-blue-500/30';
  if (count > 0) return 'bg-blue-500/15';
  return 'bg-slate-900/50';
}

function droppedColor(count: number): string {
  if (count > 100) return 'text-red-400';
  if (count > 10) return 'text-yellow-400';
  if (count > 0) return 'text-orange-400';
  return 'text-green-400';
}

const Heatmap: React.FC = () => {
  usePageTitle('Traffic Heatmap');
  const { selectedNamespace } = useNamespaceStore();
  const navigate = useNavigate();
  const [cells, setCells] = useState<HeatmapCell[]>([]);
  const [namespaces, setNamespaces] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try {
      const res = await fetchHeatmapData();
      setCells(res.data.cells ?? []);
      setNamespaces(res.data.namespaces ?? []);
    } catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const getCell = (src: string, dst: string) => cells.find((c) => c.source_namespace === src && c.destination_namespace === dst);
  const maxFlows = Math.max(...cells.map((c) => c.flow_count), 1);

  const tick = useChanged(cells);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-amber-500 to-amber-700 flex items-center justify-center shadow-lg shadow-amber-500/20"><Grid3X3 className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Traffic Heatmap</h1></div>
          <p className="text-sm text-slate-400 mt-1">Cross-namespace traffic density visualization</p>
        </div>
        <div className="flex items-center gap-3">
          {cells.length > 0 && <ExportButton data={cells as unknown as Record<string, unknown>[]} filename="heatmap" />}
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>

      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? `${namespaces.length} namespaces, ${cells.length} talking pairs.` : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'namespaces', value: tick ? namespaces.length : undefined },
          { label: 'pairs', value: tick ? cells.length : undefined },
          { label: 'flows', value: tick ? cells.reduce((n, c) => n + (c.flow_count || 0), 0).toLocaleString() : undefined },
          { label: 'dropped', value: tick ? cells.reduce((n, c) => n + (c.dropped_count || 0), 0).toLocaleString() : undefined, tone: tick ? (countTone(cells.reduce((n, c) => n + (c.dropped_count || 0), 0))) : undefined },
        ]}
      />

      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {!loading && cells.length === 0 && (
        <div className="text-center py-12 text-slate-400">
          <Grid3X3 className="w-12 h-12 mx-auto mb-3 text-amber-400" />
          <div className="font-medium text-white">No traffic data available</div>
          <div className="text-sm">Heatmap will populate once cross-namespace traffic is detected.</div>
        </div>
      )}

      {/* Heatmap grid */}
      {namespaces.length > 0 && (
        <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-6 overflow-x-auto mb-6">
          <div className="min-w-max">
            {/* Header row */}
            <div className="flex">
              <div className="w-32 flex-shrink-0" />
              {namespaces.map((ns) => (
                <div key={ns} className={`w-28 flex-shrink-0 text-center text-xs font-medium pb-2 truncate ${selectedNamespace && ns === selectedNamespace ? 'text-blue-300 font-bold' : 'text-slate-400'}`}>{ns}</div>
              ))}
            </div>
            {/* Data rows */}
            {namespaces.map((src) => (
              <div key={src} className={`flex items-center ${selectedNamespace && src === selectedNamespace ? 'bg-blue-500/5 rounded' : ''}`}>
                <div className={`w-32 flex-shrink-0 text-xs pr-3 truncate text-right ${selectedNamespace && src === selectedNamespace ? 'font-bold text-blue-300' : 'font-medium text-slate-400'}`}>{src}</div>
                {namespaces.map((dst) => {
                  const cell = getCell(src, dst);
                  return (
                    <div key={dst} className={`w-28 h-16 flex-shrink-0 p-1 cursor-pointer ${selectedNamespace && (src === selectedNamespace || dst === selectedNamespace) ? 'bg-blue-500/5' : ''}`} title={`${src} → ${dst}: ${cell?.flow_count ?? 0} flows, ${cell?.dropped_count ?? 0} drops — Click to view flows`}
                      onClick={() => navigate(`/flows?namespace=${encodeURIComponent(src)}&search=${encodeURIComponent(dst)}`)}>
                      <div className={`w-full h-full rounded-md flex flex-col items-center justify-center ${cellColor(cell?.flow_count ?? 0, maxFlows)} transition-colors hover:ring-2 hover:ring-blue-400/50 ${selectedNamespace && (src === selectedNamespace || dst === selectedNamespace) ? 'ring-1 ring-blue-400/30' : ''}`}>
                        {cell ? (
                          <>
                            <span className="text-xs font-bold text-white">{cell.flow_count >= 1000 ? `${(cell.flow_count / 1000).toFixed(1)}k` : cell.flow_count}</span>
                            {cell.dropped_count > 0 && <span className={`text-[10px] ${droppedColor(cell.dropped_count)}`}>{cell.dropped_count} drops</span>}
                          </>
                        ) : (
                          <span className="text-xs text-slate-400">-</span>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
          {/* Legend */}
          <div className="flex items-center gap-4 mt-4 pt-4 border-t border-slate-700/50 text-xs text-slate-400">
            <span>Density:</span>
            <div className="flex items-center gap-1"><div className="w-4 h-4 rounded bg-blue-500/15" /> Low</div>
            <div className="flex items-center gap-1"><div className="w-4 h-4 rounded bg-blue-500/30" /> Medium</div>
            <div className="flex items-center gap-1"><div className="w-4 h-4 rounded bg-blue-500/60" /> High</div>
            <div className="flex items-center gap-1"><div className="w-4 h-4 rounded bg-blue-500" /> Very High</div>
          </div>
        </div>
      )}

      {/* Top flows table */}
      <div className="flex items-center gap-2 mb-3"><div className="w-1 h-5 bg-gradient-to-b from-amber-400 to-orange-500 rounded-full" /><h2 className="text-lg font-semibold text-white">Top Traffic Flows</h2></div>
      <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-slate-700/50 bg-slate-900/50">
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Source</th>
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Destination</th>
              <th className="text-right px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Flow Count</th>
              <th className="text-right px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Drops</th>
              <th className="text-right px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Avg Latency</th>
            </tr>
          </thead>
          <tbody>
            {[...cells]
              .filter((c) => !selectedNamespace || c.source_namespace === selectedNamespace || c.destination_namespace === selectedNamespace)
              .sort((a, b) => b.flow_count - a.flow_count).map((c, i) => (
              <tr key={i} className="border-b border-slate-700/30 table-row-hover cursor-pointer"
                onClick={() => navigate(`/flows?namespace=${encodeURIComponent(c.source_namespace)}&search=${encodeURIComponent(c.destination_namespace ?? c.dest_namespace)}`)}>
                <td className="px-4 py-2.5 font-medium text-white">{c.source_namespace}</td>
                <td className="px-4 py-2.5 font-medium text-white">{c.destination_namespace}</td>
                <td className="px-4 py-2.5 text-right">{(c.flow_count ?? 0).toLocaleString()}</td>
                <td className={`px-4 py-2.5 text-right ${droppedColor(c.dropped_count)}`}>{c.dropped_count}</td>
                <td className="px-4 py-2.5 text-right text-slate-400">{c.avg_latency_ms} ms</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
};

export default Heatmap;
