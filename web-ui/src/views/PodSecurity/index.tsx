// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { ShieldCheck, Loader2, CheckCircle, AlertTriangle } from 'lucide-react';
import { fetchPodSecurity, PodSecurityReport } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

const LEVEL_BADGE: Record<string, string> = { privileged: 'bg-red-500/15 text-red-400 border-red-500/30', baseline: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30', restricted: 'bg-green-500/15 text-green-400 border-green-500/30' };

const PodSecurity: React.FC = () => {
  usePageTitle('Pod Security');
  const [reports, setReports] = useState<PodSecurityReport[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setReports((await fetchPodSecurity()).data.reports ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const totalViolations = reports.reduce((a, r) => a + r.violations.length, 0);

  const tick = useChanged(reports);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-green-500 to-emerald-700 flex items-center justify-center shadow-lg shadow-green-500/20"><ShieldCheck className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Pod Security Standards</h1></div>
          <p className="text-sm text-slate-400 mt-1">Pod Security Admission enforcement per namespace</p>
        </div>
        <div className="flex items-center gap-3">
          <ExportButton data={reports as unknown as Record<string, unknown>[]} filename="pod-security" />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (reports.length ? `${reports.length} pod security report${reports.length === 1 ? '' : 's'}.` : 'No pod security reports.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'Namespaces', value: tick ? reports.length : undefined },
          { label: 'Compliant Pods', value: tick ? reports.reduce((a, r) => a + r.compliant_pods, 0) : undefined },
          { label: 'Violations', value: tick ? totalViolations : undefined, tone: tick ? countTone(Number(totalViolations)) : undefined },
        ]}
      />


      {loading && reports.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {!loading && reports.length === 0 && !error && (
        <div className="text-center py-12 text-slate-400">No pod security reports available.</div>
      )}

      <div className="space-y-4">
        {reports.map((r) => {
          const pct = r.total_pods > 0 ? (r.compliant_pods / r.total_pods) * 100 : 0;
          return (
            <div key={r.namespace} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  {r.violations.length === 0 ? <CheckCircle className="w-5 h-5 text-green-400" /> : <AlertTriangle className="w-5 h-5 text-yellow-400" />}
                  <span className="font-semibold text-white">{r.namespace}</span>
                </div>
                <div className="flex gap-2">
                  <span className={`px-2 py-0.5 rounded-full text-xs border ${LEVEL_BADGE[r.enforce_level] ?? ''}`}>enforce: {r.enforce_level}</span>
                  <span className={`px-2 py-0.5 rounded-full text-xs border ${LEVEL_BADGE[r.audit_level] ?? ''}`}>audit: {r.audit_level}</span>
                </div>
              </div>
              <div className="flex justify-between text-sm mb-1">
                <span className="text-slate-400">Compliance</span>
                <span className="text-white">{r.compliant_pods} / {r.total_pods} pods ({pct.toFixed(0)}%)</span>
              </div>
              <div className="w-full h-2 rounded-full bg-slate-700 overflow-hidden mb-3">
                <div className={`h-full rounded-full ${pct >= 100 ? 'bg-green-400' : pct >= 80 ? 'bg-yellow-400' : 'bg-red-400'}`} style={{ width: `${pct}%` }} />
              </div>
              {r.violations.length > 0 && (
                <div className="space-y-1">
                  {r.violations.map((v, i) => (
                    <div key={i} className="flex items-center gap-2 text-sm p-2 rounded bg-red-500/5 border border-red-500/20">
                      <span className="text-white font-mono text-xs">{v.pod}</span>
                      <span className="text-slate-400">&mdash;</span>
                      <span className="text-red-400">{v.violation}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
};

export default PodSecurity;
