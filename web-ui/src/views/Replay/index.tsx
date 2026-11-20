// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useCallback } from 'react';
import { PlayCircle, StopCircle, Loader2, Film, Clock, HardDrive, Plus } from 'lucide-react';
import { fetchRecordings, startRecording, ReplayRecording } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const STATUS_BADGE: Record<string, string> = {
  completed: 'bg-green-500/15 text-green-400 border-green-500/30',
  recording: 'bg-red-500/15 text-red-400 border-red-500/30',
  processing: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30',
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1048576) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1048576).toFixed(1)} MB`;
}

const Replay: React.FC = () => {
  usePageTitle('Flow Replay');
  const [recordings, setRecordings] = useState<ReplayRecording[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState('');
  const [newNs, setNewNs] = useState('default');
  const [newDuration, setNewDuration] = useState('5m');
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try { setRecordings((await fetchRecordings()).data.recordings ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed to fetch recordings'); }
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const handleStart = async () => {
    if (!newName.trim()) return;
    setCreating(true); setError(null);
    try {
      await startRecording({ name: newName.trim(), namespace: newNs, duration: newDuration });
      setSuccess('Recording started'); setShowCreate(false); setNewName('');
      manualRefresh();
    } catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed to start recording'); }
    finally { setCreating(false); }
  };

  const tick = useChanged(recordings);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-violet-500 to-violet-700 flex items-center justify-center shadow-lg shadow-violet-500/20"><Film className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Flow Replay</h1></div>
          <p className="text-sm text-slate-400 mt-1">Record and replay network flows for time-travel debugging</p>
        </div>
        <div className="flex items-center gap-2">
          {recordings.length > 0 && <ExportButton data={recordings as unknown as Record<string, unknown>[]} filename="recordings" />}
          <button onClick={() => setShowCreate(true)} className="flex items-center gap-2 px-3 py-2 rounded-lg bg-gradient-to-r from-blue-600 to-blue-700 text-white text-sm hover:from-blue-500 hover:to-blue-600 transition-colors">
            <Plus className="w-4 h-4" /> New Recording
          </button>
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading} autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn((v) => !v)} intervalSecs={30} />
        </div>
      </div>

      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (recordings.length ? `${recordings.length} recording${recordings.length === 1 ? '' : 's'} ready to replay.` : 'No recordings yet.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'recordings', value: tick ? recordings.length : undefined },
          { label: 'flows', value: tick ? recordings.reduce((n, r) => n + (r.flow_count || 0), 0).toLocaleString() : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      {/* Create form */}
      {showCreate && (
        <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5 mb-6 animate-scale-in">
          <h2 className="text-sm font-semibold text-white mb-4 flex items-center gap-2"><Film className="w-4 h-4 text-violet-400" /> Start New Recording</h2>
          <div className="grid grid-cols-1 sm:grid-cols-4 gap-3">
            <input value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="Recording name"
              className="px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500" />
            <input value={newNs} onChange={(e) => setNewNs(e.target.value)} placeholder="Namespace"
              className="px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500" />
            <select value={newDuration} onChange={(e) => setNewDuration(e.target.value)}
              className="px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500">
              <option value="1m">1 minute</option>
              <option value="5m">5 minutes</option>
              <option value="15m">15 minutes</option>
              <option value="30m">30 minutes</option>
            </select>
            <div className="flex gap-2">
              <button onClick={handleStart} disabled={creating || !newName.trim()}
                className="flex-1 flex items-center justify-center gap-2 px-3 py-2 rounded-lg bg-red-600 text-white text-sm hover:bg-red-700 disabled:opacity-50 transition-colors">
                {creating ? <Loader2 className="w-4 h-4 animate-spin" /> : <PlayCircle className="w-4 h-4" />} Record
              </button>
              <button onClick={() => setShowCreate(false)} className="px-3 py-2 rounded-lg border border-slate-700/50 text-sm hover:bg-slate-700/30 transition-colors">Cancel</button>
            </div>
          </div>
        </div>
      )}

      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      {/* Recordings grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {recordings.map((rec) => (
          <div key={rec.id} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
            <div className="flex items-center justify-between mb-3">
              <div className="font-semibold text-white">{rec.name}</div>
              <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[rec.status] ?? 'bg-slate-900/50 text-slate-400 border-slate-700/50'}`}>
                {rec.status === 'recording' && <span className="animate-pulse-dot w-1.5 h-1.5 rounded-full bg-red-400 inline-block mr-1" />}
                {rec.status}
              </span>
            </div>
            <div className="grid grid-cols-2 gap-3 text-sm">
              <div className="flex items-center gap-2 text-slate-400"><Clock className="w-3.5 h-3.5" /> {rec.start_time.split('T')[1]?.slice(0, 5)} - {rec.end_time.split('T')[1]?.slice(0, 5)}</div>
              <div className="flex items-center gap-2 text-slate-400"><HardDrive className="w-3.5 h-3.5" /> {formatBytes(rec.size_bytes ?? rec.size ?? 0)}</div>
              <div><span className="text-slate-400">Namespace: </span><span className="text-white">{rec.namespace}</span></div>
              <div><span className="text-slate-400">Flows: </span><span className="text-white font-medium">{(rec.flow_count ?? 0).toLocaleString()}</span></div>
            </div>
            <div className="flex gap-2 mt-4 pt-3 border-t border-slate-700/50">
              <button className="flex-1 flex items-center justify-center gap-2 px-3 py-1.5 rounded-lg border border-slate-700/50 text-sm hover:bg-slate-700/30 transition-colors">
                <PlayCircle className="w-4 h-4" /> Replay
              </button>
              {rec.status === 'recording' && (
                <button className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-red-500/30 text-red-400 text-sm hover:bg-red-500/10 transition-colors">
                  <StopCircle className="w-4 h-4" /> Stop
                </button>
              )}
            </div>
          </div>
        ))}
        {!loading && recordings.length === 0 && (
          <div className="col-span-full text-center py-12 text-slate-400">No recordings yet. Start one to capture network flows.</div>
        )}
      </div>
    </div>
  );
};

export default Replay;
