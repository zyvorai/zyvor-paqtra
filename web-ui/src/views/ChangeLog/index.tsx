// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useCallback } from 'react';
import { Link } from 'react-router-dom';
import { History, Loader2, RotateCcw, Search, Activity } from 'lucide-react';
import { fetchChangeLog, rollbackChange, fetchChangeImpact, ChangeEntry } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const TYPE_BADGE: Record<string, string> = {
  create: 'bg-green-500/15 text-green-400 border-green-500/30',
  update: 'bg-blue-500/15 text-blue-400 border-blue-500/30',
  delete: 'bg-red-500/15 text-red-400 border-red-500/30',
};

const errorMessage = (err: unknown, fallback: string): string =>
  isAxiosError(err) ? err.response?.data?.error ?? err.response?.data?.message ?? err.message : fallback;

type ImpactResult = {
  status?: string;
  confidence?: string;
  disclaimer?: string;
  notes?: string[];
  regressions?: { path?: string; dropped_before?: number; dropped_after?: number; delta?: number }[];
  evidence_flow_ids?: string[];
  before?: { forwarded?: number; dropped?: number; total?: number };
  after?: { forwarded?: number; dropped?: number; total?: number };
  chart?: { labels?: string[]; forwarded?: number[]; dropped?: number[] };
  investigate_link?: string;
  flows_link?: string;
};

const ChangeLog: React.FC = () => {
  usePageTitle('Change Log');
  const [changes, setChanges] = useState<ChangeEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [search, setSearch] = useState('');
  const [kindFilter, setKindFilter] = useState('');
  const [nsFilter, setNsFilter] = useState('');
  const [rolling, setRolling] = useState<string | null>(null);
  const [impactId, setImpactId] = useState<string | null>(null);
  const [impact, setImpact] = useState<ImpactResult | null>(null);
  const [impactBusy, setImpactBusy] = useState(false);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try {
      setChanges((await fetchChangeLog()).data.changes ?? []);
    } catch (err) {
      setError(errorMessage(err, 'Failed'));
    }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const handleRollback = async (c: ChangeEntry) => {
    if (!window.confirm(`Roll back deployment ${c.namespace ? c.namespace + '/' : ''}${c.resource} to its previous revision?`)) return;
    setRolling(c.id);
    setError(null);
    try {
      const res = await rollbackChange(c.id);
      setSuccess(res.data?.output || 'Rollback applied');
      manualRefresh();
    } catch (err) {
      setError(errorMessage(err, 'Rollback failed'));
    } finally {
      setRolling(null);
    }
  };

  const handleImpact = async (c: ChangeEntry) => {
    setImpactBusy(true);
    setImpactId(c.id);
    setImpact(null);
    setError(null);
    try {
      const { data } = await fetchChangeImpact(c.id, {
        before: '30m',
        after: '30m',
        kind: kindFilter.trim() || undefined,
        namespace: nsFilter.trim() || c.namespace || undefined,
      });
      setImpact(data as ImpactResult);
    } catch (err) {
      setError(errorMessage(err, 'Impact analysis failed'));
    } finally {
      setImpactBusy(false);
    }
  };

  const q = search.toLowerCase();
  const filtered = changes.filter((c) => {
    if (kindFilter && !(c.type ?? '').toLowerCase().includes(kindFilter.toLowerCase())
      && !(c.resource ?? '').toLowerCase().includes(kindFilter.toLowerCase())) {
      return false;
    }
    if (nsFilter && (c.namespace ?? '').toLowerCase() !== nsFilter.toLowerCase()) return false;
    if (!search) return true;
    return [c.resource, c.author, c.diff_summary, c.message].some(
      (f) => typeof f === 'string' && f.toLowerCase().includes(q),
    );
  });

  const chartFwd = impact?.chart?.forwarded ?? [
    impact?.before?.forwarded ?? 0,
    impact?.after?.forwarded ?? 0,
  ];
  const chartDrop = impact?.chart?.dropped ?? [
    impact?.before?.dropped ?? 0,
    impact?.after?.dropped ?? 0,
  ];
  const chartMax = Math.max(1, ...chartFwd, ...chartDrop);

  const evidenceHref =
    (impact?.evidence_flow_ids ?? []).length > 0
      ? `/flows?ids=${encodeURIComponent((impact?.evidence_flow_ids ?? []).slice(0, 8).join(','))}`
      : '/flows';

  const tick = useChanged(changes);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-slate-500 to-slate-700 flex items-center justify-center shadow-lg shadow-slate-500/20">
              <History className="w-5 h-5 text-white" />
            </div>
            <h1 className="text-2xl font-bold text-white">Change Log</h1>
          </div>
          <p className="text-sm text-slate-400 mt-1">Configuration change history with rollback and impact analysis</p>
        </div>
        <div className="flex items-center gap-3">
          <ExportButton data={filtered as unknown as Record<string, unknown>[]} filename="change-log" />
          <DataFreshness
            lastUpdated={lastUpdated}
            onRefresh={manualRefresh}
            refreshing={loading}
            autoRefresh={autoRefreshOn}
            onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)}
            intervalSecs={30}
          />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (changes.length ? `${changes.length} recorded changes.` : 'No changes recorded.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'changes', value: tick ? changes.length : undefined },
          { label: 'resource kinds', value: tick ? new Set(changes.map((c) => c.type)).size : undefined },
          { label: 'namespaces', value: tick ? new Set(changes.map((c) => c.namespace).filter(Boolean)).size : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      {impact && impactId ? (
        <div className="mb-4 rounded-xl border border-slate-700/50 bg-slate-800/50 p-4">
          <div className="flex items-center justify-between mb-2">
            <h2 className="text-sm font-semibold text-white">Impact for {impactId}</h2>
            <button type="button" className="text-xs text-slate-400 hover:text-white" onClick={() => { setImpact(null); setImpactId(null); }}>
              Close
            </button>
          </div>
          <p className="text-xs text-slate-400 mb-2">
            status=<span className="text-white">{impact.status}</span> · confidence=
            <span className="text-white">{impact.confidence}</span>
          </p>
          <p className="text-xs text-slate-500 mb-2">{impact.disclaimer}</p>
          <div className="grid grid-cols-2 gap-3 text-sm mb-3">
            <div className="rounded-lg bg-slate-900/50 p-3">
              <div className="text-xs text-slate-400 mb-1">Before</div>
              <div className="text-white">fwd {chartFwd[0] ?? 0} · drop {chartDrop[0] ?? 0}</div>
              <div className="mt-2 h-2 rounded bg-slate-700 overflow-hidden flex">
                <div className="bg-emerald-500/80" style={{ width: `${((chartFwd[0] ?? 0) / chartMax) * 100}%` }} />
                <div className="bg-red-500/80" style={{ width: `${((chartDrop[0] ?? 0) / chartMax) * 100}%` }} />
              </div>
            </div>
            <div className="rounded-lg bg-slate-900/50 p-3">
              <div className="text-xs text-slate-400 mb-1">After</div>
              <div className="text-white">fwd {chartFwd[1] ?? 0} · drop {chartDrop[1] ?? 0}</div>
              <div className="mt-2 h-2 rounded bg-slate-700 overflow-hidden flex">
                <div className="bg-emerald-500/80" style={{ width: `${((chartFwd[1] ?? 0) / chartMax) * 100}%` }} />
                <div className="bg-red-500/80" style={{ width: `${((chartDrop[1] ?? 0) / chartMax) * 100}%` }} />
              </div>
            </div>
          </div>
          {(impact.notes ?? []).length > 0 ? (
            <ul className="text-xs text-amber-300/90 mb-2 list-disc pl-4">
              {(impact.notes ?? []).map((n) => (
                <li key={n}>{n}</li>
              ))}
            </ul>
          ) : null}
          {(impact.regressions ?? []).length > 0 ? (
            <div className="text-sm text-slate-300 mb-2">
              {(impact.regressions ?? []).slice(0, 8).map((r) => (
                <div key={r.path} className="font-mono text-xs py-1 border-t border-slate-700/40">
                  {r.path} · Δ drops {r.delta}
                </div>
              ))}
            </div>
          ) : (
            <p className="text-xs text-slate-400 mb-2">No new drop regressions in the window.</p>
          )}
          <p className="text-xs text-slate-400">
            <Link className="text-blue-400 hover:underline" to={evidenceHref}>
              Open evidence flows
              {(impact.evidence_flow_ids ?? []).length > 0
                ? ` (${(impact.evidence_flow_ids ?? []).length})`
                : ''}
            </Link>
            {' · '}
            <Link className="text-blue-400 hover:underline" to="/investigate">
              Path investigation
            </Link>
          </p>
        </div>
      ) : null}

      <div className="flex flex-wrap gap-2 mb-4">
        <div className="relative flex-1 min-w-[180px]">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search changes..."
            className="w-full pl-9 pr-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
          />
        </div>
        <input
          type="text"
          value={kindFilter}
          onChange={(e) => setKindFilter(e.target.value)}
          placeholder="Kind (create/CNP/…)"
          className="w-40 px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
        <input
          type="text"
          value={nsFilter}
          onChange={(e) => setNsFilter(e.target.value)}
          placeholder="Namespace"
          className="w-36 px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
      </div>

      {loading && changes.length === 0 && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {!loading && filtered.length === 0 && !error && (
        <div className="text-center py-12 text-slate-400">No changes recorded.</div>
      )}

      <div className="relative pl-6">
        <div className="absolute left-2.5 top-0 bottom-0 w-px bg-slate-700" />
        {filtered.map((c) => (
          <div key={c.id} className="relative mb-4">
            <div
              className={`absolute -left-[14px] w-3 h-3 rounded-full border-2 border-slate-800 ${
                c.type === 'create' ? 'bg-green-400' : c.type === 'delete' ? 'bg-red-400' : 'bg-blue-400'
              }`}
            />
            <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 ml-2">
              <div className="flex items-center gap-2 mb-2">
                <span className={`px-2 py-0.5 rounded-full text-xs border ${TYPE_BADGE[c.type] ?? ''}`}>{c.type}</span>
                <span className="font-mono text-sm text-white">{c.resource}</span>
                {c.namespace && <span className="px-1.5 py-0.5 rounded bg-slate-900/50 text-xs text-slate-400">{c.namespace}</span>}
                <span className="ml-auto text-xs text-slate-400">{new Date(c.timestamp).toLocaleString()}</span>
              </div>
              <div className="text-sm text-slate-400 mb-2">{c.diff_summary ?? (typeof c.message === 'string' ? c.message : '')}</div>
              <div className="flex items-center justify-between text-xs text-slate-400 gap-2 flex-wrap">
                <span>
                  By: <span className="text-white">{c.author ?? 'cluster event'}</span>
                </span>
                <div className="flex items-center gap-2">
                  {c.rolled_back && <span className="text-green-400">Rolled back</span>}
                  <button
                    type="button"
                    onClick={() => void handleImpact(c)}
                    disabled={impactBusy && impactId === c.id}
                    className="flex items-center gap-1 px-2 py-1 rounded border border-slate-700/50 hover:bg-slate-700/30 disabled:opacity-50 transition-colors text-white"
                  >
                    {impactBusy && impactId === c.id ? <Loader2 className="w-3 h-3 animate-spin" /> : <Activity className="w-3 h-3" />}
                    Analyze impact
                  </button>
                  {c.rollback_available && (
                    <button
                      type="button"
                      onClick={() => void handleRollback(c)}
                      disabled={rolling === c.id}
                      className="flex items-center gap-1 px-2 py-1 rounded border border-slate-700/50 hover:bg-slate-700/30 disabled:opacity-50 transition-colors text-white"
                    >
                      {rolling === c.id ? <Loader2 className="w-3 h-3 animate-spin" /> : <RotateCcw className="w-3 h-3" />} Rollback
                    </button>
                  )}
                </div>
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

export default ChangeLog;
