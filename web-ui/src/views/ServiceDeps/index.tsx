// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useEffect, useCallback } from 'react';
import { Workflow, RefreshCw, Loader2, ArrowRight } from 'lucide-react';
import { fetchServiceDeps, ServiceDep } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

function errorColor(rate: number): string {
  if (rate > 1) return 'text-red-400';
  if (rate > 0.5) return 'text-yellow-400';
  return 'text-green-400';
}

function latencyColor(ms: number): string {
  if (ms > 100) return 'text-red-400';
  if (ms > 50) return 'text-yellow-400';
  return 'text-green-400';
}

const ServiceDeps: React.FC = () => {
  usePageTitle('Dependencies');
  const [deps, setDeps] = useState<ServiceDep[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(async () => {
    setLoading(true); setError(null);
    try { setDeps((await fetchServiceDeps()).data.dependencies ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const services = [...new Set(deps.flatMap((d) => [d.source, d.destination]))];

  const tick = useChanged(deps);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-purple-500 to-purple-700 flex items-center justify-center shadow-lg shadow-purple-500/20"><Workflow className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Service Dependencies</h1></div>
          <p className="text-sm text-slate-400 mt-1">Service-to-service communication graph with metrics</p>
        </div>
        <button onClick={fetchData} disabled={loading} className="flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-400 hover:text-white hover:bg-slate-700/30 transition-colors">
          <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
        </button>
      </div>

      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (deps.length ? `${deps.length} service dependenc${deps.length === 1 ? 'y' : 'ies'} observed.` : 'No service dependencies yet.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'Services', value: tick ? services.length : undefined },
          { label: 'Connections', value: tick ? deps.length : undefined },
          { label: 'Requests', value: tick ? deps.reduce((a, d) => a + (d.request_count ?? 0), 0).toLocaleString() : undefined },
          { label: 'High Error', value: tick ? deps.filter((d) => d.error_rate > 1).length : undefined, tone: tick ? countTone(Number(deps.filter((d) => d.error_rate > 1).length)) : undefined },
        ]}
      />

      {/* Service summary */}

      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {/* Dependency cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {deps.map((d, i) => (
          <div key={i} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 card-glow transition-all hover:scale-[1.01]">
            <div className="flex items-center gap-3 mb-3">
              <span className="font-semibold text-white">{d.source}</span>
              <ArrowRight className="w-4 h-4 text-slate-400 flex-shrink-0" />
              <span className="font-semibold text-white">{d.destination}</span>
              <span className="ml-auto px-2 py-0.5 rounded border border-slate-700/50 text-xs text-slate-400">{d.protocol}:{d.port}</span>
            </div>
            <div className="grid grid-cols-4 gap-3 text-sm">
              <div>
                <div className="text-xs text-slate-400">Requests</div>
                <div className="font-medium text-white">{d.request_count ?? d.request_rate ?? '—'}</div>
              </div>
              <div>
                <div className="text-xs text-slate-400">Error Rate</div>
                <div className={`font-medium ${errorColor(d.error_rate)}`}>{d.error_rate}%</div>
              </div>
              {d.latency_p50 != null && (
                <div>
                  <div className="text-xs text-slate-400">P50 Latency</div>
                  <div className={`font-medium ${latencyColor(d.latency_p50)}`}>{d.latency_p50} ms</div>
                </div>
              )}
              {d.latency_p99 != null && (
                <div>
                  <div className="text-xs text-slate-400">P99 Latency</div>
                  <div className={`font-medium ${latencyColor(d.latency_p99)}`}>{d.latency_p99} ms</div>
                </div>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

export default ServiceDeps;
