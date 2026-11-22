// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useEffect, useCallback } from 'react';
import { Users, RefreshCw, Loader2, Search, Shield, User, UserCog } from 'lucide-react';
import { fetchRBACBindings, RBACBinding } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const KIND_ICON: Record<string, React.ReactNode> = { User: <User className="w-4 h-4" />, ServiceAccount: <UserCog className="w-4 h-4" />, Group: <Users className="w-4 h-4" /> };
const KIND_BADGE: Record<string, string> = { ClusterRole: 'bg-purple-500/15 text-purple-400 border-purple-500/30', Role: 'bg-blue-500/15 text-blue-400 border-blue-500/30' };

const RBACVisualizer: React.FC = () => {
  usePageTitle('RBAC');
  const [bindings, setBindings] = useState<RBACBinding[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');

  const fetchData = useCallback(async () => {
    setLoading(true); setError(null);
    try { setBindings((await fetchRBACBindings()).data.bindings ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const filtered = search ? bindings.filter((b) => (b.subject ?? b.subjects?.[0]?.name ?? '').toLowerCase().includes(search.toLowerCase()) || (b.role ?? '').toLowerCase().includes(search.toLowerCase()) || (b.permissions ?? []).some((p) => (typeof p === 'string' ? p : '').includes(search.toLowerCase()))) : bindings;

  const tick = useChanged(bindings);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20"><Users className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">RBAC Visualizer</h1></div>
          <p className="text-sm text-slate-400 mt-1">Kubernetes RBAC bindings and permission overview</p>
        </div>
        <button onClick={fetchData} disabled={loading} className="flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-400 hover:text-white hover:bg-slate-700/30 transition-colors"><RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} /></button>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (bindings.length ? `${bindings.length} RBAC binding${bindings.length === 1 ? '' : 's'}.` : 'No RBAC bindings.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'bindings', value: tick ? bindings.length : undefined },
          { label: 'subjects', value: tick ? new Set(bindings.map((b) => b.subject)).size : undefined },
          { label: 'roles', value: tick ? new Set(bindings.map((b) => b.role)).size : undefined },
          { label: 'namespaces', value: tick ? new Set(bindings.map((b) => b.namespace).filter(Boolean)).size : undefined },
        ]}
      />

      <div className="relative mb-4"><Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400" /><input type="text" value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search subjects, roles, permissions..." className="w-full pl-9 pr-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500" /></div>

      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      <div className="space-y-3">
        {filtered.map((b, i) => (
          <div key={i} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4">
            <div className="flex items-center gap-3 mb-3">
              <div className="text-slate-400">{KIND_ICON[b.subject_kind ?? b.subjects?.[0]?.kind ?? ''] ?? KIND_ICON.User}</div>
              <div className="flex-1">
                <div className="font-medium text-white">{b.subject ?? b.subjects?.[0]?.name ?? ''}</div>
                <div className="text-xs text-slate-400">{b.subject_kind ?? b.subjects?.[0]?.kind ?? ''}</div>
              </div>
              <div className="flex items-center gap-2">
                <Shield className="w-4 h-4 text-slate-400" />
                <div className="text-right">
                  <div className="font-medium text-white">{b.role}</div>
                  <span className={`px-2 py-0.5 rounded-full text-xs border ${KIND_BADGE[b.role_kind ?? 'Role'] ?? ''}`}>{b.role_kind ?? 'Role'}</span>
                </div>
              </div>
            </div>
            <div className="flex items-center gap-2 text-xs text-slate-400 mb-2">
              <span>Namespace: </span><span className="px-1.5 py-0.5 rounded bg-slate-900/50 text-white">{b.namespace ?? b.subjects?.[0]?.namespace ?? 'cluster-wide'}</span>
            </div>
            <div className="flex flex-wrap gap-1">
              {(b.permissions ?? []).map((p) => (
                <span key={p} className="px-2 py-0.5 rounded bg-slate-900/50 text-xs text-slate-400">{p}</span>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

export default RBACVisualizer;
