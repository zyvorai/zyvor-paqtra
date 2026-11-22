// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { Hexagon, Loader2, Lock, LockOpen, RotateCcw, Timer, Zap } from 'lucide-react';
import { fetchMeshServices, MeshService } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const ServiceMeshView: React.FC = () => {
  usePageTitle('Service Mesh');
  const [services, setServices] = useState<MeshService[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setServices((await fetchMeshServices()).data.services ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const mtlsCount = services.filter((s) => s.mtls).length;

  const tick = useChanged(services);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20"><Hexagon className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Service Mesh</h1></div>
          <p className="text-sm text-slate-400 mt-1">Cilium service mesh configuration and traffic policies</p>
        </div>
        <div className="flex items-center gap-3">
          <ExportButton data={services as unknown as Record<string, unknown>[]} filename="service-mesh" />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (services.length ? `${services.length} mesh service${services.length === 1 ? '' : 's'}.` : 'No mesh services.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'Services', value: tick ? services.length : undefined },
          { label: 'mTLS Enabled', value: tick ? mtlsCount : undefined },
          { label: 'Circuit Breakers', value: tick ? services.filter((s) => s.circuit_breaker).length : undefined },
        ]}
      />


      {loading && services.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {!loading && services.length === 0 && !error && (
        <div className="text-center py-12 text-slate-400">No service mesh services found.</div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {services.map((s) => (
          <div key={`${s.namespace}/${s.name}`} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4">
            <div className="flex items-center justify-between mb-3">
              <span className="font-semibold text-white">{s.name}</span>
              <span className="px-2 py-0.5 rounded border border-slate-700/50 text-xs text-slate-400">{s.namespace}</span>
            </div>
            <div className="grid grid-cols-2 gap-2 text-sm">
              <div className="flex items-center gap-1">{s.mtls ? <Lock className="w-3.5 h-3.5 text-green-400" /> : <LockOpen className="w-3.5 h-3.5 text-yellow-400" />}<span className={s.mtls ? 'text-green-400' : 'text-yellow-400'}>{s.mtls ? 'mTLS' : 'Plain'}</span></div>
              <div className="flex items-center gap-1"><span className="px-1.5 py-0.5 rounded bg-slate-900/50 text-xs">{s.protocol}</span></div>
              <div className="flex items-center gap-1"><RotateCcw className="w-3.5 h-3.5 text-slate-400" /><span className="text-white">{s.retries} retries</span></div>
              <div className="flex items-center gap-1"><Timer className="w-3.5 h-3.5 text-slate-400" /><span className="text-white">{s.timeout_ms}ms</span></div>
              <div className="flex items-center gap-1"><Zap className="w-3.5 h-3.5 text-slate-400" /><span className={s.circuit_breaker ? 'text-green-400' : 'text-slate-400'}>{s.circuit_breaker ? 'CB On' : 'CB Off'}</span></div>
              <div className="text-xs text-slate-400">{s.traffic_policy}</div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

export default ServiceMeshView;
