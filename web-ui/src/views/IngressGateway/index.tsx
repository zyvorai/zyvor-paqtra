// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { Globe, Loader2, Lock, LockOpen, ExternalLink } from 'lucide-react';
import { fetchIngressRoutes } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyRoute = Record<string, any>;

function getHosts(r: AnyRoute): string[] {
  if (Array.isArray(r.hosts)) return r.hosts;
  if (r.host) return [r.host];
  if (r.tls?.hosts) return r.tls.hosts;
  return [];
}

function isTls(r: AnyRoute): boolean {
  if (typeof r.tls === 'boolean') return r.tls;
  if (r.tls?.enabled) return true;
  return false;
}

function getPaths(r: AnyRoute): { path: string; backend: string; port: number }[] {
  return (r.paths ?? []).map((p: AnyRoute) => ({
    path: p.path ?? '/',
    backend: p.backend ?? p.backend_service ?? '-',
    port: p.port ?? p.backend_port ?? 0,
  }));
}

const STATUS_BADGE: Record<string, string> = { active: 'bg-green-500/15 text-green-400 border-green-500/30', pending: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30', error: 'bg-red-500/15 text-red-400 border-red-500/30' };

const IngressGateway: React.FC = () => {
  usePageTitle('Ingress');
  const [routes, setRoutes] = useState<AnyRoute[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setRoutes((await fetchIngressRoutes()).data.routes ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const tick = useChanged(routes);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20"><Globe className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Ingress & Gateway</h1></div>
          <p className="text-sm text-slate-400 mt-1">Ingress and Gateway API route configuration</p>
        </div>
        <div className="flex items-center gap-3">
          <ExportButton data={routes as unknown as Record<string, unknown>[]} filename="ingress-routes" />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (routes.length ? `${routes.length} ingress and gateway route${routes.length === 1 ? '' : 's'}.` : 'No ingress or gateway routes.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'routes', value: tick ? routes.length : undefined },
          { label: 'with TLS', value: tick ? routes.filter((r) => isTls(r)).length : undefined },
          { label: 'without TLS', value: tick ? routes.filter((r) => !isTls(r)).length : undefined, tone: tick ? (countTone(routes.filter((r) => !isTls(r)).length)) : undefined },
        ]}
      />
      {loading && routes.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {routes.map((r, idx) => {
          const hosts = getHosts(r);
          const tls = isTls(r);
          const paths = getPaths(r);
          const status = r.status ?? 'active';
          return (
            <div key={r.name ?? idx} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  {tls ? <Lock className="w-4 h-4 text-green-400" /> : <LockOpen className="w-4 h-4 text-yellow-400" />}
                  <span className="font-semibold text-white">{r.name ?? 'unknown'}</span>
                </div>
                <div className="flex items-center gap-2">
                  {r.type && <span className="px-2 py-0.5 rounded border border-slate-700/50 text-xs text-slate-400">{r.type}</span>}
                  <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[status] ?? 'bg-slate-900/50 text-slate-400 border-slate-700/50'}`}>{status}</span>
                </div>
              </div>
              {hosts.length > 0 && (
                <div className="text-sm mb-3">
                  <span className="text-slate-400">Hosts: </span>
                  {hosts.map((h) => (
                    <span key={h} className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-slate-900/50 text-xs text-white mr-1"><ExternalLink className="w-3 h-3" />{h}</span>
                  ))}
                </div>
              )}
              <div className="text-xs text-slate-400 mb-1">
                {r.class && <>Class: <span className="text-white">{r.class}</span> &bull; </>}
                Namespace: <span className="text-white">{r.namespace ?? '-'}</span>
              </div>
              {paths.length > 0 && (
                <div className="border-t border-slate-700/50 mt-3 pt-3">
                  <div className="text-xs text-slate-400 mb-2">Path Rules</div>
                  {paths.map((p, i) => (
                    <div key={i} className="flex items-center gap-2 py-1 text-sm">
                      <span className="font-mono text-white">{p.path}</span>
                      <span className="text-slate-400">&rarr;</span>
                      <span className="text-white">{p.backend}:{p.port}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
        {!loading && routes.length === 0 && !error && <div className="col-span-full text-center py-12 text-slate-400">No ingress routes configured.</div>}
      </div>
    </div>
  );
};

export default IngressGateway;
