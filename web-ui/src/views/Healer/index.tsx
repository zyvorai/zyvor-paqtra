// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { HeartPulse, Loader2, Wrench, AlertOctagon, AlertTriangle, Info } from 'lucide-react';
import { fetchHealerProblems, applyHealerFix, HealerProblem } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

const SEV_BADGE: Record<string, string> = {
  critical: 'bg-red-500/15 text-red-400 border-red-500/30',
  high: 'bg-orange-500/15 text-orange-400 border-orange-500/30',
  medium: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30',
  low: 'bg-blue-500/15 text-blue-400 border-blue-500/30',
};

const SEV_ICON: Record<string, React.ReactNode> = {
  critical: <AlertOctagon className="w-5 h-5 text-red-400" />,
  high: <AlertTriangle className="w-5 h-5 text-orange-400" />,
  medium: <AlertTriangle className="w-5 h-5 text-yellow-400" />,
  low: <Info className="w-5 h-5 text-blue-400" />,
};

const STATUS_BADGE: Record<string, string> = {
  open: 'bg-red-500/15 text-red-400 border-red-500/30',
  investigating: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30',
  fixed: 'bg-green-500/15 text-green-400 border-green-500/30',
  fix_applied: 'bg-green-500/15 text-green-400 border-green-500/30',
};

const errorMessage = (err: unknown, fallback: string): string =>
  isAxiosError(err) ? err.response?.data?.error ?? err.response?.data?.message ?? err.message : fallback;

const Healer: React.FC = () => {
  usePageTitle('Network Healer');
  const [problems, setProblems] = useState<HealerProblem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [fixing, setFixing] = useState<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setProblems((await fetchHealerProblems()).data.problems ?? []); }
    catch (err) { setError(errorMessage(err, 'Failed')); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const handleFix = async (id: string) => {
    setFixing(id); setError(null);
    try { const res = await applyHealerFix(id); setSuccess(res.data?.message ?? 'Fix applied'); manualRefresh(); }
    catch (err) { setError(errorMessage(err, 'Fix failed')); }
    finally { setFixing(null); }
  };

  const tick = useChanged(problems);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-rose-500 to-rose-700 flex items-center justify-center shadow-lg shadow-rose-500/20"><HeartPulse className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Network Healer</h1></div>
          <p className="text-sm text-slate-400 mt-1">Automatic problem detection with proposed fixes</p>
        </div>
        <div className="flex items-center gap-3">
          {problems.length > 0 && <ExportButton data={problems as unknown as Record<string, unknown>[]} filename="healer-problems" />}
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>

      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (problems.length ? `${problems.filter((p) => !/fix|resolv/i.test(p.status || '')).length} open problem${problems.filter((p) => !/fix|resolv/i.test(p.status || '')).length === 1 ? '' : 's'} with proposed fixes.` : 'No problems detected.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'problems', value: tick ? problems.length : undefined },
          { label: 'open', value: tick ? problems.filter((p) => !/fix|resolv/i.test(p.status || '')).length : undefined, tone: tick ? (countTone(problems.filter((p) => !/fix|resolv/i.test(p.status || '')).length)) : undefined },
          { label: 'high or critical', value: tick ? problems.filter((p) => /crit|high/i.test(p.severity || "")).length : undefined, tone: tick ? (countTone(problems.filter((p) => /crit|high/i.test(p.severity || "")).length)) : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      <div className="space-y-4">
        {problems.map((p) => (
          <div key={p.id} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
            <div className="flex items-start gap-4">
              <div className="mt-1">{SEV_ICON[p.severity] ?? SEV_ICON.low}</div>
              <div className="flex-1">
                <div className="flex items-center gap-2 mb-1">
                  <span className={`px-2 py-0.5 rounded-full text-xs border ${SEV_BADGE[p.severity] ?? ''}`}>{p.severity.toUpperCase()}</span>
                  <span className="px-2 py-0.5 rounded border border-slate-700/50 text-xs text-slate-400">{p.type.replace(/_/g, ' ')}</span>
                  <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[p.status] ?? ''}`}>{p.status}</span>
                </div>
                <h3 className="text-white font-medium mb-2">{p.description}</h3>
                <div className="text-sm text-slate-400 mb-3">
                  <span>Namespace: </span><span className="text-white">{p.namespace}</span>
                  <span className="mx-2">|</span>
                  <span>Affected: </span><span className="text-white">{p.affected_pods.join(', ')}</span>
                </div>
                <div className="p-3 rounded-lg bg-slate-900/50 border border-slate-700/50">
                  <div className="text-xs font-medium text-slate-400 mb-1">Proposed Fix</div>
                  <div className="text-sm text-white">{p.proposed_fix}</div>
                </div>
              </div>
              <button onClick={() => handleFix(p.id)}
                disabled={fixing === p.id || p.status === 'fixed' || p.status === 'fix_applied' || p.auto_fixable === false}
                title={p.auto_fixable === false ? 'No automated fix for this problem: apply the proposed change manually' : undefined}
                className="flex items-center gap-2 px-4 py-2 rounded-lg bg-green-600 text-white text-sm hover:bg-green-700 disabled:opacity-50 transition-colors whitespace-nowrap">
                {fixing === p.id ? <Loader2 className="w-4 h-4 animate-spin" /> : <Wrench className="w-4 h-4" />}
                {p.status === 'fix_applied' ? 'Fix applied' : p.auto_fixable === false ? 'Manual fix' : 'Apply Fix'}
              </button>
            </div>
          </div>
        ))}
        {!loading && problems.length === 0 && (
          <div className="text-center py-12 text-slate-400">
            <HeartPulse className="w-12 h-12 mx-auto mb-3 text-green-400" />
            <div className="font-medium text-white">All healthy</div>
            <div className="text-sm">No problems detected in your network.</div>
          </div>
        )}
      </div>
    </div>
  );
};

export default Healer;
