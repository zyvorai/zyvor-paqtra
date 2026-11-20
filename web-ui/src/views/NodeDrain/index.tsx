// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useEffect, useCallback } from 'react';
import { ServerOff, RefreshCw, Loader2, Undo2, ShieldOff } from 'lucide-react';
import { fetchNodeDrainStatus, drainNode, uncordonNode, NodeDrainStatus } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import { useAuthStore } from '../../stores/authStore';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const STATUS_BADGE: Record<string, string> = { ready: 'bg-green-500/15 text-green-400 border-green-500/30', draining: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30', cordoned: 'bg-orange-500/15 text-orange-400 border-orange-500/30', drained: 'bg-blue-500/15 text-blue-400 border-blue-500/30' };

const NodeDrain: React.FC = () => {
  usePageTitle('Node Drain');
  const userRole = useAuthStore((s) => s.role);
  const isAdmin = userRole === 'admin';
  const [nodes, setNodes] = useState<NodeDrainStatus[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [acting, setActing] = useState<string | null>(null);
  const [confirmDrain, setConfirmDrain] = useState<string | null>(null);

  const fetchData = useCallback(async () => {
    setLoading(true); setError(null);
    try { setNodes((await fetchNodeDrainStatus()).data.nodes ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const confirmAndDrain = (node: string) => setConfirmDrain(node);

  const handleDrain = async (node: string) => {
    setConfirmDrain(null);
    setActing(node); setError(null);
    try { await drainNode(node); setSuccess(`Drain initiated for ${node}`); fetchData(); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
    finally { setActing(null); }
  };

  const handleUncordon = async (node: string) => {
    setActing(node); setError(null);
    try { await uncordonNode(node); setSuccess(`Node ${node} uncordoned`); fetchData(); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Failed'); }
    finally { setActing(null); }
  };

  const tick = useChanged(nodes);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-red-500 to-red-700 flex items-center justify-center shadow-lg shadow-red-500/20"><ServerOff className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Node Drain</h1></div>
          <p className="text-sm text-slate-400 mt-1">Gracefully drain nodes for maintenance</p>
        </div>
        <button onClick={fetchData} disabled={loading} className="flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-400 hover:text-white hover:bg-slate-700/30 transition-colors"><RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin' : ''}`} /></button>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? `${nodes.length} node${nodes.length === 1 ? '' : 's'}, ${nodes.filter((n) => n.cordon).length} cordoned.` : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'nodes', value: tick ? nodes.length : undefined },
          { label: 'cordoned', value: tick ? nodes.filter((n) => n.cordon).length : undefined },
          { label: 'pods remaining', value: tick ? nodes.reduce((n, x) => n + (x.pods_remaining || 0), 0) : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}
      {!isAdmin && (
        <div className="mb-4 p-3 rounded-lg bg-yellow-500/10 border border-yellow-500/30 text-yellow-400 text-sm">
          Admin role required to drain or uncordon nodes.
        </div>
      )}
      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        {nodes.map((n) => (
          <div key={n.node} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
            <div className="flex items-center justify-between mb-4">
              <span className="font-semibold text-white text-lg">{n.node}</span>
              <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[n.status] ?? ''}`}>{n.status}</span>
            </div>
            <div className="space-y-2 text-sm mb-4">
              <div className="flex justify-between"><span className="text-slate-400">Cordoned</span><span className={n.cordon ? 'text-orange-400' : 'text-green-400'}>{n.cordon ? 'Yes' : 'No'}</span></div>
              <div className="flex justify-between"><span className="text-slate-400">Pods Evicted</span><span className="text-white">{n.pods_evicted}</span></div>
              <div className="flex justify-between"><span className="text-slate-400">Pods Remaining</span><span className="text-white">{n.pods_remaining}</span></div>
              {n.started_at && <div className="flex justify-between"><span className="text-slate-400">Started</span><span className="text-white">{new Date(n.started_at).toLocaleTimeString()}</span></div>}
            </div>
            <div className="flex gap-2">
              {n.status === 'ready' && (
                <button onClick={() => confirmAndDrain(n.node)} disabled={acting === n.node || !isAdmin} className="flex-1 flex items-center justify-center gap-2 px-3 py-2 rounded-lg bg-red-600 text-white text-sm hover:bg-red-600/90 disabled:opacity-50 disabled:cursor-not-allowed transition-colors" title={isAdmin ? '' : 'Admin role required'}>
                  {acting === n.node ? <Loader2 className="w-4 h-4 animate-spin" /> : <ShieldOff className="w-4 h-4" />} Drain
                </button>
              )}
              {(n.status === 'cordoned' || n.status === 'drained') && (
                <button onClick={() => handleUncordon(n.node)} disabled={acting === n.node || !isAdmin} className="flex-1 flex items-center justify-center gap-2 px-3 py-2 rounded-lg bg-green-600 text-white text-sm hover:bg-green-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors" title={isAdmin ? '' : 'Admin role required'}>
                  {acting === n.node ? <Loader2 className="w-4 h-4 animate-spin" /> : <Undo2 className="w-4 h-4" />} Uncordon
                </button>
              )}
              {n.status === 'draining' && <div className="flex-1 text-center py-2 text-yellow-400 text-sm animate-pulse-dot">Draining...</div>}
            </div>
          </div>
        ))}
      </div>

      {confirmDrain && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50" role="dialog" aria-modal="true" aria-label="Confirm node drain">
          <div className="bg-slate-800 border border-slate-700 rounded-xl p-6 max-w-md w-full mx-4">
            <h3 className="text-lg font-semibold text-white mb-2">Confirm Node Drain</h3>
            <p className="text-sm text-slate-400 mb-4">
              Are you sure you want to drain node <span className="text-white font-mono">{confirmDrain}</span>? This will evict all pods from the node.
            </p>
            <div className="flex gap-3 justify-end">
              <button onClick={() => setConfirmDrain(null)} className="px-4 py-2 rounded-lg border border-slate-600 text-slate-300 text-sm hover:bg-slate-700 transition-colors">Cancel</button>
              <button onClick={() => handleDrain(confirmDrain)} className="px-4 py-2 rounded-lg bg-red-600 text-white text-sm hover:bg-red-700 transition-colors">Drain Node</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

export default NodeDrain;
