// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState } from 'react';
import {
  Bird,
  Play,
  Loader2,
  CheckCircle,
  XCircle,
  ArrowRight,
  TrendingUp,
} from 'lucide-react';
import { fetchCanaryStatus } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

interface CanaryData {
  id: string;
  name: string;
  status: string;
  current_weight: number;
  target_weight: number;
  success_rate: number;
  error_rate: number;
  latency_p99: number;
}

const Canary: React.FC = () => {
  usePageTitle('Canary');
  const [canaryId, setCanaryId] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [data, setData] = useState<CanaryData | null>(null);

  const handleFetch = async () => {
    if (!canaryId.trim()) return;
    setLoading(true); setError(null); setData(null);
    try {
      const res = await fetchCanaryStatus(canaryId.trim());
      const raw = res.data;
      const canary = raw.canary ?? raw;
      setData({
        id: canary.id ?? '',
        name: canary.name ?? canary.id ?? '',
        status: canary.status ?? '',
        current_weight: canary.current_weight ?? canary.traffic_split?.canary ?? 0,
        target_weight: canary.target_weight ?? 100,
        success_rate: canary.success_rate ?? canary.metrics?.success_rate ?? 0,
        error_rate: canary.error_rate ?? canary.metrics?.error_count ?? 0,
        latency_p99: canary.latency_p99 ?? canary.metrics?.latency_p99_ms ?? 0,
      });
    } catch (err) {
      setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed to fetch canary status');
    } finally {
      setLoading(false);
    }
  };

  const tick = useChanged(data);
  return (
    <div className="netra-page">
      <div className="mb-6">
        <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-emerald-500 to-green-700 flex items-center justify-center shadow-lg shadow-emerald-500/20"><Bird className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Canary Deployments</h1></div>
        <p className="text-sm text-slate-400 mt-1">Progressive traffic shifting with auto-promote and auto-rollback</p>
      </div>

      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (data ? `${data.name} at ${data.current_weight}% of traffic, target ${data.target_weight}%.` : undefined) : undefined}
        tick={tick}
        live={false}
        error={error || undefined}
        figures={[
          { label: 'status', value: tick ? data?.status ?? "—" : undefined },
          { label: 'weight', value: tick ? data ? `${data.current_weight}%` : "—" : undefined },
          { label: 'success rate', value: tick ? data ? `${data.success_rate}%` : "—" : undefined },
          { label: 'error rate', value: tick ? data ? `${data.error_rate}%` : "—" : undefined },
        ]}
      />

      {/* Lookup */}
      <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-6 mb-6">
        <h2 className="text-sm font-semibold text-white mb-4">Check Canary Status</h2>
        <div className="flex gap-3">
          <input value={canaryId} onChange={(e) => setCanaryId(e.target.value)} placeholder="Enter canary deployment ID"
            className="flex-1 px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-white text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
            onKeyDown={(e) => e.key === 'Enter' && handleFetch()} />
          <button onClick={handleFetch} disabled={loading || !canaryId.trim()}
            className="flex items-center gap-2 px-4 py-2 rounded-lg bg-gradient-to-r from-blue-600 to-blue-700 text-white text-sm hover:from-blue-500 hover:to-blue-600 disabled:opacity-50 transition-colors">
            {loading ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
            Check
          </button>
        </div>
      </div>

      {/* How it works */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4 mb-6">
        {[
          { step: '1', title: 'Deploy', desc: 'Deploy canary version alongside stable' },
          { step: '2', title: 'Shift', desc: 'Gradually shift traffic (5% -> 10% -> 25% -> 50% -> 100%)' },
          { step: '3', title: 'Monitor', desc: 'Track success rate, latency, errors' },
          { step: '4', title: 'Decide', desc: 'Auto-promote (>99% success) or rollback (<90%)' },
        ].map((s) => (
          <div key={s.step} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 card-glow transition-all hover:scale-[1.01]">
            <div className="flex items-center gap-2 mb-2">
              <div className="w-7 h-7 rounded-full bg-blue-500/20 flex items-center justify-center text-blue-400 font-bold text-xs">{s.step}</div>
              <span className="font-semibold text-white text-sm">{s.title}</span>
            </div>
            <p className="text-xs text-slate-400">{s.desc}</p>
          </div>
        ))}
      </div>

      {/* Result */}
      {data && (
        <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-6 animate-scale-in">
          <div className="page-chrome flex items-center justify-between mb-6">
            <h2 className="text-lg font-semibold text-white">{data.name}</h2>
            <span className={`px-3 py-1 rounded-full text-sm font-medium border ${
              data.status === 'promoting' ? 'bg-green-500/15 text-green-400 border-green-500/30' :
              data.status === 'rolling_back' ? 'bg-red-500/15 text-red-400 border-red-500/30' :
              'bg-blue-500/15 text-blue-400 border-blue-500/30'
            }`}>
              {data.status.replace(/_/g, ' ')}
            </span>
          </div>

          {/* Progress bar */}
          <div className="mb-6">
            <div className="flex items-center justify-between text-sm mb-2">
              <span className="text-slate-400">Traffic Weight</span>
              <span className="text-white font-medium">{data.current_weight}% <ArrowRight className="w-3 h-3 inline" /> {data.target_weight}%</span>
            </div>
            <div className="w-full h-3 rounded-full bg-slate-700 overflow-hidden">
              <div className="h-full rounded-full bg-blue-600 transition-all duration-500" style={{ width: `${data.current_weight}%` }} />
            </div>
          </div>

          {/* Metrics */}
          <div className="grid grid-cols-3 gap-4">
            <div className="rounded-xl border border-slate-700/50 p-4 text-center">
              <div className="flex items-center justify-center gap-1 mb-1">
                {data.success_rate >= 99 ? <CheckCircle className="w-4 h-4 text-green-400" /> : <XCircle className="w-4 h-4 text-yellow-400" />}
              </div>
              <div className={`text-2xl font-bold ${data.success_rate >= 99 ? 'text-green-400' : data.success_rate >= 90 ? 'text-yellow-400' : 'text-red-400'}`}>
                {(data.success_rate ?? 0).toFixed(1)}%
              </div>
              <div className="text-xs text-slate-400 mt-1">Success Rate</div>
            </div>
            <div className="rounded-xl border border-slate-700/50 p-4 text-center">
              <TrendingUp className="w-4 h-4 text-slate-400 mx-auto mb-1" />
              <div className="text-2xl font-bold text-white">{(data.error_rate ?? 0).toFixed(2)}%</div>
              <div className="text-xs text-slate-400 mt-1">Error Rate</div>
            </div>
            <div className="rounded-xl border border-slate-700/50 p-4 text-center">
              <TrendingUp className="w-4 h-4 text-slate-400 mx-auto mb-1" />
              <div className="text-2xl font-bold text-white">{data.latency_p99} ms</div>
              <div className="text-xs text-slate-400 mt-1">P99 Latency</div>
            </div>
          </div>

          {/* Thresholds */}
          <div className="mt-4 p-3 rounded-lg bg-slate-900/50 border border-slate-700/50 text-xs text-slate-400">
            <strong>Auto-promote:</strong> success rate &ge; 99% &bull; <strong>Auto-rollback:</strong> success rate &lt; 90%
          </div>
        </div>
      )}
    </div>
  );
};

export default Canary;
