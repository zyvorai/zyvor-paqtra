// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useEffect, useCallback } from 'react';
import {
  FlaskConical,
  RefreshCw,
  Play,
  Loader2,
  Zap,
  Wifi,
  Clock,
  Bug,
  AlertTriangle,
} from 'lucide-react';
import { fetchChaosExperiments, runChaosExperiment, apiErrorMessage } from '../../services/api';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import { useAuthStore } from '../../stores/authStore';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

interface Experiment {
  id: string;
  name: string;
  type: string;
  status: string;
  target_namespace: string;
  duration: string;
  created_at: string;
  [key: string]: unknown;
}

const PRESETS = [
  { name: 'Network Partition', type: 'network_partition', icon: <Wifi className="w-5 h-5" />, desc: 'Simulate complete network isolation between namespaces', color: 'text-red-400' },
  { name: 'Latency Spike', type: 'latency_spike', icon: <Clock className="w-5 h-5" />, desc: 'Inject 500ms latency on all egress traffic', color: 'text-yellow-400' },
  { name: 'DNS Outage', type: 'dns_outage', icon: <Bug className="w-5 h-5" />, desc: 'Block DNS resolution to simulate DNS failures', color: 'text-orange-400' },
  { name: 'Packet Loss', type: 'packet_loss', icon: <AlertTriangle className="w-5 h-5" />, desc: 'Drop 30% of packets on target interface', color: 'text-purple-400' },
  { name: 'Bandwidth Limit', type: 'bandwidth_limit', icon: <Zap className="w-5 h-5" />, desc: 'Throttle bandwidth to 1Mbps on target pods', color: 'text-blue-400' },
];

const STATUS_BADGE: Record<string, string> = {
  running: 'bg-green-500/15 text-green-400 border-green-500/30',
  completed: 'bg-blue-500/15 text-blue-400 border-blue-500/30',
  failed: 'bg-red-500/15 text-red-400 border-red-500/30',
  scheduled: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30',
};

/** Flip to true once the backend action is implemented. */
const RUN_AVAILABLE = false;
const NOT_AVAILABLE_MSG = 'Fault injection is not implemented yet: experiments cannot be run, so no network faults are injected.';

const Chaos: React.FC = () => {
  usePageTitle('Chaos Engineering');
  const userRole = useAuthStore((s) => s.role);
  const isAdmin = userRole === 'admin';
  const [experiments, setExperiments] = useState<Experiment[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [running, setRunning] = useState<string | null>(null);
  const [confirmExp, setConfirmExp] = useState<{ type: string; name: string } | null>(null);

  const fetchData = useCallback(async () => {
    setLoading(true); setError(null);
    try { setExperiments((await fetchChaosExperiments()).data.experiments ?? []); }
    catch (err) { setError(apiErrorMessage(err, 'Failed to load experiments')); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const confirmAndRun = (type: string, name: string) => setConfirmExp({ type, name });

  const handleRun = async (type: string, name: string) => {
    setConfirmExp(null);
    setRunning(type); setError(null);
    try {
      await runChaosExperiment({ type, target_namespace: 'default', duration: '30s' });
      setSuccess(`${name} experiment started`);
      fetchData();
    } catch (err) { setError(apiErrorMessage(err, 'Experiment failed')); }
    finally { setRunning(null); }
  };

  const tick = useChanged(experiments);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-pink-500 to-pink-700 flex items-center justify-center shadow-lg shadow-pink-500/20"><FlaskConical className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Chaos Engineering</h1></div>
          <p className="text-sm text-slate-400 mt-1">Network fault injection with tc-netem presets</p>
        </div>
        <button onClick={fetchData} disabled={loading} className="flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-400 hover:text-white hover:bg-slate-700/30 transition-colors">
          <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} />
        </button>
      </div>

      {!RUN_AVAILABLE && <div className="mb-4 p-3 rounded-lg bg-amber-500/10 border border-amber-500/30 text-amber-300 text-sm">{NOT_AVAILABLE_MSG}</div>}
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (experiments.length ? `${experiments.length} chaos experiment${experiments.length === 1 ? '' : 's'} on record.` : 'No chaos experiments yet.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'experiments', value: tick ? experiments.length : undefined },
          { label: 'running', value: tick ? experiments.filter((e) => /run|activ/i.test(e.status || '')).length : undefined },
          { label: 'namespaces', value: tick ? new Set(experiments.map((e) => e.target_namespace).filter(Boolean)).size : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      {!isAdmin && (
        <div className="mb-4 p-3 rounded-lg bg-yellow-500/10 border border-yellow-500/30 text-yellow-400 text-sm">
          Admin role required to run chaos experiments.
        </div>
      )}

      {/* Experiment presets */}
      <div className="flex items-center gap-2 mb-3"><div className="w-1 h-5 bg-gradient-to-b from-pink-400 to-rose-500 rounded-full" /><h2 className="text-lg font-semibold text-white">Experiment Presets</h2></div>
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
        {PRESETS.map((p) => (
          <div key={p.type} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5 flex flex-col">
            <div className="flex items-center gap-3 mb-3">
              <div className={`${p.color}`}>{p.icon}</div>
              <span className="font-semibold text-white">{p.name}</span>
            </div>
            <p className="text-sm text-slate-400 mb-4 flex-1">{p.desc}</p>
            <button
              onClick={() => confirmAndRun(p.type, p.name)}
              disabled={running === p.type || !isAdmin || !RUN_AVAILABLE}
              className="flex items-center justify-center gap-2 w-full px-3 py-2 rounded-lg border border-slate-700/50 text-sm hover:bg-slate-700/30 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              title={!RUN_AVAILABLE ? NOT_AVAILABLE_MSG : isAdmin ? '' : 'Admin role required'}
            >
              {running === p.type ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
              {running === p.type ? 'Starting...' : 'Run Experiment'}
            </button>
          </div>
        ))}
      </div>

      {/* History */}
      <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
        <div className="px-5 py-4 border-b border-slate-700/50 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-pink-500 to-pink-700 flex items-center justify-center shadow-lg shadow-pink-500/20">
              <FlaskConical className="w-4 h-4 text-white" />
            </div>
            <h2 className="text-lg font-semibold text-white">Experiment History</h2>
          </div>
          <span className="text-xs font-medium text-slate-400 bg-slate-700/50 px-2.5 py-1 rounded-full">
            {experiments.length} experiments
          </span>
        </div>
        {loading && <div className="flex justify-center p-3"><Loader2 className="w-5 h-5 animate-spin text-blue-400" /></div>}
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-slate-700/50 bg-slate-900/50">
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Name</th>
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Type</th>
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Namespace</th>
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Duration</th>
              <th className="text-left px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider">Status</th>
            </tr>
          </thead>
          <tbody>
            {experiments.length === 0 && !loading ? (
              <tr><td colSpan={5} className="px-4 py-12 text-center text-slate-400">No experiments yet. Run a preset to get started.</td></tr>
            ) : experiments.map((e) => (
              <tr key={e.id} className="border-b border-slate-700/30 table-row-hover">
                <td className="px-4 py-2.5 font-medium text-white">{e.name}</td>
                <td className="px-4 py-2.5"><span className="px-2 py-0.5 rounded border border-slate-700/50 text-xs">{e.type ?? e.experiment_type ?? '-'}</span></td>
                <td className="px-4 py-2.5 text-slate-400">{e.target_namespace}</td>
                <td className="px-4 py-2.5 text-slate-400">{e.duration ?? (e.duration_secs ? `${e.duration_secs}s` : '-')}</td>
                <td className="px-4 py-2.5">
                  <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[e.status] ?? 'bg-slate-900/50 text-slate-400 border-slate-700/50'}`}>{e.status}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {confirmExp && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50" role="dialog" aria-modal="true" aria-label="Confirm chaos experiment">
          <div className="bg-slate-800 border border-slate-700 rounded-xl p-6 max-w-md w-full mx-4">
            <h3 className="text-lg font-semibold text-white mb-2">Confirm Chaos Experiment</h3>
            <p className="text-sm text-slate-400 mb-4">
              Are you sure you want to run <span className="text-white font-semibold">{confirmExp.name}</span>? This will inject network faults into the default namespace.
            </p>
            <div className="flex gap-3 justify-end">
              <button onClick={() => setConfirmExp(null)} className="px-4 py-2 rounded-lg border border-slate-600 text-slate-300 text-sm hover:bg-slate-700 transition-colors">Cancel</button>
              <button onClick={() => handleRun(confirmExp.type, confirmExp.name)} className="px-4 py-2 rounded-lg bg-pink-600 text-white text-sm hover:bg-pink-700 transition-colors">Run Experiment</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default Chaos;
