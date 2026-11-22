// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import { Target, Loader2, CheckCircle, AlertTriangle, XCircle, HelpCircle, Trash2, Plus } from 'lucide-react';
import { fetchSLOs, createSLO, deleteSLO, apiErrorMessage, SLOTarget } from '../../services/api';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

function statusIcon(s: string) {
  if (s === 'met') return <CheckCircle className="w-5 h-5 text-green-400" />;
  if (s === 'at_risk') return <AlertTriangle className="w-5 h-5 text-yellow-400" />;
  if (s === 'no_data') return <HelpCircle className="w-5 h-5 text-slate-400" />;
  return <XCircle className="w-5 h-5 text-red-400" />;
}

function budgetColor(remaining: number, total: number) {
  const pct = (remaining / total) * 100;
  return pct > 50 ? 'bg-green-400' : pct > 20 ? 'bg-yellow-400' : 'bg-red-400';
}

const WINDOWS = ['24h', '7d', '30d'];
const inputCls = 'px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500';

const SLODashboard: React.FC = () => {
  usePageTitle('SLO Dashboard');
  const [slos, setSlos] = useState<SLOTarget[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);
  const [showForm, setShowForm] = useState(false);
  const [saving, setSaving] = useState(false);
  const [name, setName] = useState('');
  const [target, setTarget] = useState('99.9');
  const [windowSel, setWindowSel] = useState('30d');
  const [namespace, setNamespace] = useState('');

  const fetchData = useCallback(async () => {
    setError(null);
    try { setSlos((await fetchSLOs()).data.slos ?? []); }
    catch (err) { setError(apiErrorMessage(err, 'Failed')); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const handleCreate = async () => {
    setSaving(true); setError(null);
    try {
      await createSLO({ name: name.trim(), target: Number(target), window: windowSel, namespace: namespace.trim() || undefined });
      setSuccess('SLO created'); setShowForm(false); setName(''); setNamespace(''); manualRefresh();
    } catch (err) { setError(apiErrorMessage(err, 'Failed to create SLO')); }
    finally { setSaving(false); }
  };

  const handleDelete = async (s: SLOTarget) => {
    if (!s.id || !window.confirm(`Delete SLO "${s.name}"?`)) return;
    setError(null);
    try { await deleteSLO(s.id); setSuccess('SLO deleted'); manualRefresh(); }
    catch (err) { setError(apiErrorMessage(err, 'Failed to delete SLO')); }
  };

  const met = slos.filter((s) => s.status === 'met').length;
  const breached = slos.filter((s) => s.status === 'breached').length;

  const tick = useChanged(slos);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-green-500 to-green-700 flex items-center justify-center shadow-lg shadow-green-500/20"><Target className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">SLO Dashboard</h1></div>
          <p className="text-sm text-slate-400 mt-1">Availability objectives, projected from the latest flow sample</p>
        </div>
        <div className="flex items-center gap-3">
          <button onClick={() => setShowForm((v) => !v)} className="flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-300 hover:text-white hover:bg-slate-700/30 transition-colors"><Plus className="w-4 h-4" /> New SLO</button>
          <ExportButton data={slos as unknown as Record<string, unknown>[]} filename="slo-dashboard" />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (slos.length ? `${slos.length} objective${slos.length === 1 ? '' : 's'} tracked.` : 'No objectives defined.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'Total SLOs', value: tick ? slos.length : undefined },
          { label: 'Meeting Target', value: tick ? met : undefined },
          { label: 'Breached', value: tick ? breached : undefined, tone: tick ? countTone(Number(breached)) : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      {showForm && (
        <div className="mb-6 rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 flex flex-wrap items-end gap-3">
          <label className="text-xs text-slate-400 flex flex-col gap-1">Name<input className={inputCls} value={name} onChange={(e) => setName(e.target.value)} placeholder="checkout availability" /></label>
          <label className="text-xs text-slate-400 flex flex-col gap-1">Target %<input className={`${inputCls} w-24`} value={target} onChange={(e) => setTarget(e.target.value)} inputMode="decimal" /></label>
          <label className="text-xs text-slate-400 flex flex-col gap-1">Window<select className={inputCls} value={windowSel} onChange={(e) => setWindowSel(e.target.value)}>{WINDOWS.map((w) => <option key={w} value={w}>{w}</option>)}</select></label>
          <label className="text-xs text-slate-400 flex flex-col gap-1">Namespace (optional)<input className={inputCls} value={namespace} onChange={(e) => setNamespace(e.target.value)} placeholder="all namespaces" /></label>
          <button onClick={handleCreate} disabled={saving || !name.trim() || !target} className="flex items-center gap-2 px-4 py-2 rounded-lg bg-green-600 text-white text-sm hover:bg-green-700 disabled:opacity-50 transition-colors">{saving && <Loader2 className="w-4 h-4 animate-spin" />}Create</button>
        </div>
      )}


      {loading && slos.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {!loading && slos.length === 0 && !error && (
        <div className="text-center py-12 text-slate-400">No SLO targets yet. Use “New SLO” to add one.</div>
      )}

      <div className="space-y-4">
        {slos.map((s) => {
          const hasData = s.current !== null && s.budget_remaining !== null;
          const remaining = s.budget_remaining ?? 0;
          const budgetPct = hasData && s.budget_total > 0 ? (remaining / s.budget_total) * 100 : 0;
          return (
            <div key={s.id ?? s.name} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
              <div className="flex items-center gap-3 mb-4">
                {statusIcon(s.status)}
                <div className="flex-1">
                  <div className="font-semibold text-white">{s.name}</div>
                  <div className="text-xs text-slate-400">{s.service} &bull; {s.metric} &bull; {s.window}</div>
                </div>
                <div className="text-right">
                  <div className="text-2xl font-bold text-white">{s.current === null ? '—' : `${s.current.toFixed(2)}%`}</div>
                  <div className="text-xs text-slate-400">Target: {s.target}%</div>
                </div>
                {s.id && <button onClick={() => handleDelete(s)} title="Delete SLO" className="p-2 rounded-lg text-slate-400 hover:text-red-400 hover:bg-slate-700/30 transition-colors"><Trash2 className="w-4 h-4" /></button>}
              </div>
              <div className="mb-2">
                <div className="flex justify-between text-sm mb-1">
                  <span className="text-slate-400">Projected error budget</span>
                  <span className="text-white">{hasData ? `${remaining.toFixed(1)} / ${s.budget_total.toFixed(1)} min remaining` : 'no data in the latest sample'}</span>
                </div>
                <div className="w-full h-2.5 rounded-full bg-slate-700 overflow-hidden">
                  <div className={`h-full rounded-full ${hasData ? budgetColor(remaining, s.budget_total) : 'bg-slate-600'} transition-all`} style={{ width: `${Math.max(budgetPct, 0)}%` }} />
                </div>
              </div>
              {s.measurement && <div className="text-xs text-slate-500 mt-2">{s.measurement}</div>}
            </div>
          );
        })}
      </div>
    </div>
  );
};

export default SLODashboard;
