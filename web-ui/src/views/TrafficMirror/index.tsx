// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useEffect, useCallback } from 'react';
import { Copy, RefreshCw, Loader2, Plus, Trash2, X } from 'lucide-react';
import { fetchMirrorRules, createMirrorRule, deleteMirrorRule, MirrorRule, apiErrorMessage } from '../../services/api';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const STATUS_BADGE: Record<string, string> = { active: 'bg-green-500/15 text-green-400 border-green-500/30', paused: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30' };

/** Flip to true once the backend action is implemented. */
const CREATE_AVAILABLE = false;
const NOT_AVAILABLE_MSG = 'Traffic mirroring is not implemented yet: rules cannot be created because nothing would be mirrored.';

const TrafficMirror: React.FC = () => {
  usePageTitle('Traffic Mirroring');
  const [rules, setRules] = useState<MirrorRule[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState('');
  const [newSrc, setNewSrc] = useState('');
  const [newDst, setNewDst] = useState('');
  const [newMirror, setNewMirror] = useState('');
  const [creating, setCreating] = useState(false);

  const fetchData = useCallback(async () => {
    setLoading(true); setError(null);
    try { setRules((await fetchMirrorRules()).data.rules ?? []); }
    catch (err) { setError(apiErrorMessage(err, 'Failed')); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const handleCreate = async () => {
    if (!newName || !newSrc || !newDst || !newMirror) return;
    setCreating(true); setError(null);
    try { await createMirrorRule({ name: newName, source_selector: newSrc, destination: newDst, mirror_to: newMirror }); setSuccess('Mirror rule created'); setShowCreate(false); setNewName(''); fetchData(); }
    catch (err) { setError(apiErrorMessage(err, 'Failed')); }
    finally { setCreating(false); }
  };

  const handleDelete = async (id: string) => {
    setError(null);
    try { await deleteMirrorRule(id); setSuccess('Rule deleted'); fetchData(); }
    catch (err) { setError(apiErrorMessage(err, 'Failed')); }
  };

  const tick = useChanged(rules);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-purple-500 to-purple-700 flex items-center justify-center shadow-lg shadow-purple-500/20"><Copy className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Traffic Mirroring</h1></div>
          <p className="text-sm text-slate-400 mt-1">Shadow traffic to debug services without impacting production</p>
        </div>
        <div className="flex gap-2">
          <button onClick={fetchData} disabled={loading} className="flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-400 hover:text-white hover:bg-slate-700/30 transition-colors"><RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} /></button>
          <button onClick={() => setShowCreate(true)} className="flex items-center gap-2 px-3 py-2 rounded-lg bg-gradient-to-r from-blue-600 to-blue-700 text-white text-sm hover:from-blue-500 hover:to-blue-600 transition-colors"><Plus className="w-4 h-4" /> New Rule</button>
        </div>
      </div>
      {!CREATE_AVAILABLE && <div className="mb-4 p-3 rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-300 text-sm">{NOT_AVAILABLE_MSG}</div>}
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (rules.length ? `${rules.length} mirror rule${rules.length === 1 ? '' : 's'}.` : 'No mirror rules.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'rules', value: tick ? rules.length : undefined },
          { label: 'active', value: tick ? rules.filter((r) => /activ/i.test(r.status || '')).length : undefined },
          { label: 'packets mirrored', value: tick ? rules.reduce((n, r) => n + (r.mirrored_packets || 0), 0).toLocaleString() : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      {showCreate && (
        <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5 mb-6 animate-scale-in">
          <div className="flex items-center justify-between mb-4"><h2 className="text-sm font-semibold text-white">Create Mirror Rule</h2><button onClick={() => setShowCreate(false)} className="text-slate-400 hover:text-white"><X className="w-4 h-4" /></button></div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 mb-4">
            <input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="Rule name" className="px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500" />
            <input value={newSrc} onChange={(e) => setNewSrc(e.target.value)} placeholder="Source selector (e.g. app=frontend)" className="px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500" />
            <input value={newDst} onChange={(e) => setNewDst(e.target.value)} placeholder="Destination (e.g. api-gateway:8080)" className="px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500" />
            <input value={newMirror} onChange={(e) => setNewMirror(e.target.value)} placeholder="Mirror to (e.g. shadow-service:8080)" className="px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500" />
          </div>
          <button onClick={handleCreate} disabled={creating || !newName || !CREATE_AVAILABLE} title={CREATE_AVAILABLE ? undefined : NOT_AVAILABLE_MSG} className="flex items-center gap-2 px-4 py-2 rounded-lg bg-gradient-to-r from-blue-600 to-blue-700 text-white text-sm hover:from-blue-500 hover:to-blue-600 disabled:opacity-50 transition-colors">
            {creating ? <Loader2 className="w-4 h-4 animate-spin" /> : <Plus className="w-4 h-4" />} Create
          </button>
        </div>
      )}

      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}
      <div className="space-y-3">
        {rules.map((r) => (
          <div key={r.id} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 flex items-center gap-4">
            <div className="flex-1">
              <div className="flex items-center gap-2 mb-1">
                <span className="font-semibold text-white">{r.name}</span>
                <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[r.status] ?? ''}`}>{r.status}</span>
              </div>
              <div className="text-sm text-slate-400">
                <span className="font-mono text-white">{r.source_selector ?? String(r.source ?? '-')}</span> &rarr; <span className="font-mono text-white">{typeof r.destination === 'string' ? r.destination : String(r.destination ?? '-')}</span> &rarr; <span className="font-mono text-blue-400">{r.mirror_to ?? (r.mirror?.service ? `${r.mirror.namespace}/${r.mirror.service}:${r.mirror.port}` : '-')}</span>
              </div>
              <div className="text-xs text-slate-400 mt-1">{(r.mirrored_packets ?? r.stats?.packets_mirrored ?? 0).toLocaleString()} packets mirrored</div>
            </div>
            <button onClick={() => handleDelete(r.id)} className="p-2 rounded-lg hover:bg-slate-700/30 text-slate-400 hover:text-red-400 transition-colors"><Trash2 className="w-4 h-4" /></button>
          </div>
        ))}
        {!loading && rules.length === 0 && <div className="text-center py-12 text-slate-400">No mirror rules configured.</div>}
      </div>
    </div>
  );
};

export default TrafficMirror;
