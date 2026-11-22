// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useEffect, useCallback } from 'react';
import { Download, RefreshCw, Loader2, Plus, Trash2, X } from 'lucide-react';
import { fetchExportConfigs, createExportConfig, deleteExportConfig, ExportConfig } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const STATUS_BADGE: Record<string, string> = { active: 'bg-green-500/15 text-green-400 border-green-500/30', paused: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30', error: 'bg-red-500/15 text-red-400 border-red-500/30' };
const FORMAT_BADGE: Record<string, string> = { json: 'bg-blue-500/15 text-blue-400 border-blue-500/30', csv: 'bg-green-500/15 text-green-400 border-green-500/30', syslog: 'bg-purple-500/15 text-purple-400 border-purple-500/30', s3: 'bg-orange-500/15 text-orange-400 border-orange-500/30' };

const FlowExporter: React.FC = () => {
  usePageTitle('Flow Exporter');
  const [configs, setConfigs] = useState<ExportConfig[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState('');
  const [newFormat, setNewFormat] = useState('json');
  const [newDest, setNewDest] = useState('');
  const [creating, setCreating] = useState(false);

  const fetchData = useCallback(async () => {
    setLoading(true); setError(null);
    try { setConfigs((await fetchExportConfigs()).data.configs ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const handleCreate = async () => {
    if (!newName || !newDest) return;
    setCreating(true); setError(null);
    try { await createExportConfig({ name: newName, format: newFormat, destination: newDest }); setSuccess('Export config created'); setShowCreate(false); setNewName(''); fetchData(); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
    finally { setCreating(false); }
  };

  const handleDelete = async (id: string) => {
    try { await deleteExportConfig(id); setSuccess('Deleted'); fetchData(); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
  };

  const tick = useChanged(configs);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20"><Download className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Flow Exporter</h1></div>
          <p className="text-sm text-slate-400 mt-1">Export network flows to external destinations</p>
        </div>
        <div className="flex gap-2">
          <button onClick={fetchData} disabled={loading} className="flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-400 hover:text-white hover:bg-slate-700/30 transition-colors"><RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} /></button>
          <button onClick={() => setShowCreate(true)} className="flex items-center gap-2 px-3 py-2 rounded-lg bg-gradient-to-r from-blue-600 to-blue-700 text-white text-sm hover:from-blue-500 hover:to-blue-600 transition-colors"><Plus className="w-4 h-4" /> New Export</button>
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (configs.length ? `${configs.length} flow export${configs.length === 1 ? '' : 's'} configured.` : 'No flow exports configured.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'exports', value: tick ? configs.length : undefined },
          { label: 'active', value: tick ? configs.filter((c) => /activ|run/i.test(c.status || '')).length : undefined },
          { label: 'flows exported', value: tick ? configs.reduce((n, c) => n + (c.exported_count || 0), 0).toLocaleString() : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      {showCreate && (
        <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5 mb-6 animate-scale-in">
          <div className="flex items-center justify-between mb-4"><h2 className="text-sm font-semibold text-white">Create Export Config</h2><button onClick={() => setShowCreate(false)} className="text-slate-400 hover:text-white"><X className="w-4 h-4" /></button></div>
          <div className="grid grid-cols-1 sm:grid-cols-4 gap-3">
            <input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="Export name" className="px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500" />
            <select value={newFormat} onChange={(e) => setNewFormat(e.target.value)} className="px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500">
              <option value="json">JSON</option><option value="csv">CSV</option><option value="syslog">Syslog</option><option value="s3">S3</option>
            </select>
            <input value={newDest} onChange={(e) => setNewDest(e.target.value)} placeholder="Destination URL" className="px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white font-mono focus:outline-none focus:ring-2 focus:ring-blue-500" />
            <button onClick={handleCreate} disabled={creating || !newName || !newDest} className="flex items-center justify-center gap-2 px-4 py-2 rounded-lg bg-gradient-to-r from-blue-600 to-blue-700 text-white text-sm hover:from-blue-500 hover:to-blue-600 disabled:opacity-50 transition-colors">
              {creating ? <Loader2 className="w-4 h-4 animate-spin" /> : <Plus className="w-4 h-4" />} Create
            </button>
          </div>
        </div>
      )}

      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}
      <div className="space-y-3">
        {configs.map((c) => (
          <div key={c.id} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 flex items-center gap-4">
            <div className="flex-1">
              <div className="flex items-center gap-2 mb-1">
                <span className="font-semibold text-white">{c.name}</span>
                <span className={`px-2 py-0.5 rounded-full text-xs border ${FORMAT_BADGE[c.format] ?? ''}`}>{c.format.toUpperCase()}</span>
                <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[c.status] ?? ''}`}>{c.status}</span>
              </div>
              <div className="text-sm text-slate-400 font-mono">{c.destination}</div>
              <div className="text-xs text-slate-400 mt-1">{(c.exported_count ?? 0).toLocaleString()} flows exported &bull; Last: {c.last_export ? new Date(c.last_export).toLocaleString() : 'never'}</div>
            </div>
            <button onClick={() => handleDelete(c.id)} className="p-2 rounded-lg hover:bg-slate-700/30 text-slate-400 hover:text-red-400 transition-colors"><Trash2 className="w-4 h-4" /></button>
          </div>
        ))}
        {!loading && configs.length === 0 && <div className="text-center py-12 text-slate-400">No export configs. Create one to start exporting flows.</div>}
      </div>
    </div>
  );
};

export default FlowExporter;
