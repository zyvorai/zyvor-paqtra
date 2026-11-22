// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useEffect, useCallback } from 'react';
import { FileCode, RefreshCw, Loader2, Copy, Play, Search, Tag } from 'lucide-react';
import { fetchPolicyTemplates, applyTemplate, PolicyTemplate } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const CAT_COLOR: Record<string, string> = {
  security: 'bg-red-500/15 text-red-400 border-red-500/30',
  connectivity: 'bg-green-500/15 text-green-400 border-green-500/30',
  observability: 'bg-blue-500/15 text-blue-400 border-blue-500/30',
  l7: 'bg-purple-500/15 text-purple-400 border-purple-500/30',
};

const PolicyTemplates: React.FC = () => {
  usePageTitle('Policy Templates');
  const [templates, setTemplates] = useState<PolicyTemplate[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [search, setSearch] = useState('');
  const [expanded, setExpanded] = useState<string | null>(null);
  const [applying, setApplying] = useState<string | null>(null);

  const fetchData = useCallback(async () => {
    setLoading(true); setError(null);
    try { setTemplates((await fetchPolicyTemplates()).data.templates ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const handleApply = async (id: string) => {
    setApplying(id); setError(null);
    try { await applyTemplate(id, 'default'); setSuccess('Template applied to default namespace'); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Apply failed'); }
    finally { setApplying(null); }
  };

  const filtered = search ? templates.filter((t) => t.name.toLowerCase().includes(search.toLowerCase()) || t.description.toLowerCase().includes(search.toLowerCase()) || t.tags.some((tag) => tag.includes(search.toLowerCase()))) : templates;

  const tick = useChanged(templates);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-indigo-500 to-indigo-700 flex items-center justify-center shadow-lg shadow-indigo-500/20"><FileCode className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Policy Templates</h1></div>
          <p className="text-sm text-slate-400 mt-1">Pre-built CiliumNetworkPolicy templates</p>
        </div>
        <button onClick={fetchData} disabled={loading} className="flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-400 hover:text-white hover:bg-slate-700/30 transition-colors">
          <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
        </button>
      </div>

      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? `${templates.length} policy templates in ${new Set(templates.map((t) => t.category)).size} categories.` : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'templates', value: tick ? templates.length : undefined },
          { label: 'categories', value: tick ? new Set(templates.map((t) => t.category)).size : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      <div className="relative mb-4">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400" />
        <input type="text" value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search templates..."
          className="w-full pl-9 pr-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500" />
      </div>

      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {filtered.map((t) => (
          <div key={t.id} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
            <div className="flex items-center justify-between mb-2">
              <h3 className="font-semibold text-white">{t.name}</h3>
              <span className={`px-2 py-0.5 rounded-full text-xs border ${CAT_COLOR[t.category] ?? 'bg-slate-900/50 text-slate-400 border-slate-700/50'}`}>{t.category}</span>
            </div>
            <p className="text-sm text-slate-400 mb-3">{t.description}</p>
            <div className="flex flex-wrap gap-1 mb-3">
              {t.tags.map((tag) => (
                <span key={tag} className="flex items-center gap-1 px-2 py-0.5 rounded bg-slate-900/50 text-xs text-slate-400"><Tag className="w-3 h-3" />{tag}</span>
              ))}
            </div>
            {expanded === t.id && (
              <pre className="p-3 rounded-xl bg-slate-950 border border-slate-800 text-xs font-mono text-white overflow-auto max-h-48 mb-3 animate-scale-in">{t.yaml}</pre>
            )}
            <div className="flex gap-2">
              <button onClick={() => setExpanded(expanded === t.id ? null : t.id)}
                className="flex-1 flex items-center justify-center gap-2 px-3 py-1.5 rounded-lg border border-slate-700/50 text-sm hover:bg-slate-700/30 transition-colors">
                <Copy className="w-4 h-4" /> {expanded === t.id ? 'Hide' : 'View'} YAML
              </button>
              <button onClick={() => handleApply(t.id)} disabled={applying === t.id}
                className="flex-1 flex items-center justify-center gap-2 px-3 py-1.5 rounded-lg bg-gradient-to-r from-blue-600 to-blue-700 text-white text-sm hover:from-blue-500 hover:to-blue-600 disabled:opacity-50 transition-colors">
                {applying === t.id ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />} Apply
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

export default PolicyTemplates;
