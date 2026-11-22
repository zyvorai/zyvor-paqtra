// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useCallback } from 'react';
import axios from 'axios';
import { BellRing, Loader2, Bell, BellOff, Clock, VolumeX, Plus, Pencil, Trash2 } from 'lucide-react';
import {
  fetchAlertRules, fetchAlertHistory, toggleAlertRule, deleteAlertRule, fetchChannels, fetchSilences, apiErrorMessage,
  AlertRule, AlertEvent, AlertSilence, NotificationChannel,
} from '../../services/api';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import DataFreshness from '../../components/DataFreshness';
import ExportButton from '../../components/ExportButton';
import { ChannelsPanel, SilencesPanel } from './NotificationSettings';
import AlertRuleForm from './AlertRuleForm';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

const SEV_BADGE: Record<string, string> = { critical: 'bg-red-500/15 text-red-400 border-red-500/30', high: 'bg-orange-500/15 text-orange-400 border-orange-500/30', warning: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30' };
const STATUS_BADGE: Record<string, string> = { firing: 'bg-red-500/15 text-red-400 border-red-500/30', resolved: 'bg-green-500/15 text-green-400 border-green-500/30' };

type Tab = 'rules' | 'history' | 'channels' | 'silences';

const isForbidden = (r: PromiseSettledResult<unknown>): boolean =>
  r.status === 'rejected' && axios.isAxiosError(r.reason) && r.reason.response?.status === 403;

const Alerts: React.FC = () => {
  usePageTitle('Alerts');
  const [rules, setRules] = useState<AlertRule[]>([]);
  const [history, setHistory] = useState<AlertEvent[]>([]);
  const [channels, setChannels] = useState<NotificationChannel[]>([]);
  const [silences, setSilences] = useState<AlertSilence[]>([]);
  const [forbidden, setForbidden] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const [tab, setTab] = useState<Tab>('rules');
  const [toggling, setToggling] = useState<string | null>(null);
  // 'new' = create form open; a rule id = that rule is being edited.
  const [editing, setEditing] = useState<string | 'new' | null>(null);
  const [autoRefreshOn, setAutoRefreshOn] = useState(true);

  const fetchData = useCallback(async () => {
    setError(null);
    try {
      const [r, h] = await Promise.all([fetchAlertRules(), fetchAlertHistory()]);
      setRules(r.data.rules ?? []);
      setHistory(h.data.alerts ?? []);
    } catch (err) { setError(apiErrorMessage(err, 'Failed')); }
    // Channels and silences are admin-only: a refusal must not break Rules and History.
    const [c, s] = await Promise.allSettled([fetchChannels(), fetchSilences()]);
    setForbidden(isForbidden(c) || isForbidden(s));
    if (c.status === 'fulfilled') setChannels(c.value.data.channels ?? []);
    if (s.status === 'fulfilled') setSilences(s.value.data.silences ?? []);
  }, []);

  const { lastUpdated, refreshing: loading, manualRefresh } = useAutoRefresh(fetchData, 30000, autoRefreshOn);

  const onMessage = (kind: 'success' | 'error', text: string) => {
    if (kind === 'success') { setError(null); setSuccess(text); } else { setSuccess(null); setError(text); }
  };

  const handleToggle = async (rule: AlertRule) => {
    setToggling(rule.id);
    try { await toggleAlertRule(rule.id); onMessage('success', `${rule.name} ${rule.enabled ? 'disabled' : 'enabled'}`); manualRefresh(); }
    catch (err) { onMessage('error', apiErrorMessage(err, 'Failed to update rule')); }
    finally { setToggling(null); }
  };

  const handleDelete = async (rule: AlertRule) => {
    if (!window.confirm(`Delete alert rule "${rule.name}"? It will stop firing.`)) return;
    try {
      try { await deleteAlertRule(rule.id); }
      catch (err) {
        // Seeded defaults are refused unless forced; confirm again rather than silently forcing.
        const builtin = axios.isAxiosError(err) && err.response?.status === 400 && /built-in/.test(String(err.response.data?.error ?? ''));
        if (!builtin || !window.confirm(`"${rule.name}" is a built-in rule. Delete it anyway?`)) throw err;
        await deleteAlertRule(rule.id, true);
      }
      onMessage('success', `${rule.name} deleted`);
      manualRefresh();
    } catch (err) { onMessage('error', apiErrorMessage(err, 'Failed to delete rule')); }
  };

  const handleSaved = (text: string) => { setEditing(null); onMessage('success', text); manualRefresh(); };

  // Silence expiry is evaluated against the current time on every render.
  // eslint-disable-next-line react-hooks/purity
  const now = Date.now();
  const activeSilences = silences.filter((s) => new Date(s.until).getTime() > now);
  const silencedUntil = (ruleId: string): string | null => {
    const hit = activeSilences.find((s) => s.rule_id === null || s.rule_id === ruleId);
    return hit ? hit.until : null;
  };

  const firing = history.filter((a) => a.status === 'firing').length;
  const exportData = ({ rules, history, channels, silences } as Record<Tab, unknown[]>)[tab];
  const tabBtn = (t: Tab, label: string) => (
    <button onClick={() => setTab(t)} className={`px-4 py-2 rounded-md text-sm transition-colors ${tab === t ? 'bg-slate-800/50 text-white shadow' : 'text-slate-400'}`}>{label}</button>
  );

  const tick = useChanged(rules);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-amber-500 to-amber-700 flex items-center justify-center shadow-lg shadow-amber-500/20"><BellRing className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Alerts</h1></div>
          <p className="text-sm text-slate-400 mt-1">Alert rules, notification channels and silences</p>
        </div>
        <div className="flex items-center gap-3">
          {firing > 0 && <span className="px-3 py-1.5 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm animate-pulse-dot">{firing} firing</span>}
          <ExportButton data={exportData as Record<string, unknown>[]} filename={`alerts-${tab}`} />
          <DataFreshness lastUpdated={lastUpdated} onRefresh={manualRefresh} refreshing={loading}
            autoRefresh={autoRefreshOn} onAutoRefreshToggle={() => setAutoRefreshOn(v => !v)} intervalSecs={30} />
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (rules.length ? `${rules.filter((r) => r.enabled).length} of ${rules.length} alert rules enabled.` : 'No alert rules yet.') : undefined}
        tick={tick}
        error={error || undefined}
        figures={[
          { label: 'rules', value: tick ? rules.length : undefined },
          { label: 'enabled', value: tick ? rules.filter((r) => r.enabled).length : undefined },
          { label: 'firing', value: tick ? history.filter((h) => /fir|activ|trigger/i.test(h.status || '')).length : undefined, tone: tick ? (countTone(history.filter((h) => /fir|activ|trigger/i.test(h.status || '')).length)) : undefined },
          { label: 'channels', value: tick ? channels.length : undefined },
          { label: 'silences', value: tick ? silences.length : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}
      <div className="flex gap-1 mb-4 p-1 rounded-lg bg-slate-900/50 w-fit">
        {tabBtn('rules', `Rules (${rules.length})`)}
        {tabBtn('history', `History (${history.length})`)}
        {tabBtn('channels', `Channels (${channels.length})`)}
        {tabBtn('silences', `Silences (${activeSilences.length})`)}
      </div>
      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}
      {tab === 'rules' && (
        <div className="space-y-3">
          <div className="flex justify-end">
            <button onClick={() => setEditing(editing === 'new' ? null : 'new')} className="flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-300 hover:text-white hover:bg-slate-700/30 transition-colors"><Plus className="w-4 h-4" /> New rule</button>
          </div>
          {editing === 'new' && <AlertRuleForm onSaved={handleSaved} onCancel={() => setEditing(null)} />}
          {!loading && rules.length === 0 && <div className="text-center py-12 text-slate-400">No alert rules found</div>}
          {rules.map((r) => {
            const until = silencedUntil(r.id);
            if (editing === r.id) return <AlertRuleForm key={r.id} rule={r} onSaved={handleSaved} onCancel={() => setEditing(null)} />;
            return (
              <div key={r.id} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 flex items-center gap-4">
                {r.enabled ? <Bell className="w-5 h-5 text-blue-400" /> : <BellOff className="w-5 h-5 text-slate-400" />}
                <div className="flex-1">
                  <div className="flex items-center gap-2 mb-1">
                    <span className="font-medium text-white">{r.name}</span>
                    <span className={`px-2 py-0.5 rounded-full text-xs border ${SEV_BADGE[r.severity] ?? ''}`}>{r.severity}</span>
                    {!r.enabled && <span className="px-2 py-0.5 rounded border border-slate-700/50 text-xs text-slate-400">disabled</span>}
                    {until && <span className="flex items-center gap-1 px-2 py-0.5 rounded border border-slate-700/50 text-xs text-slate-400"><VolumeX className="w-3 h-3" /> silenced until {new Date(until).toLocaleTimeString()}</span>}
                  </div>
                  <div className="text-sm text-slate-400 font-mono">{r.condition}</div>
                  <div className="flex items-center gap-3 mt-1 text-xs text-slate-400">
                    <span>Triggered {r.trigger_count ?? 0}x</span>
                    {r.last_triggered && <span className="flex items-center gap-1"><Clock className="w-3 h-3" /> {new Date(r.last_triggered).toLocaleDateString()}</span>}
                  </div>
                </div>
                <button onClick={() => setEditing(r.id)} aria-label={`Edit ${r.name}`} className="p-2 rounded-lg border border-slate-700/50 text-slate-300 hover:text-white hover:bg-slate-700/30 transition-colors"><Pencil className="w-4 h-4" /></button>
                <button onClick={() => handleDelete(r)} aria-label={`Delete ${r.name}`} className="p-2 rounded-lg border border-slate-700/50 text-slate-300 hover:text-red-400 hover:bg-slate-700/30 transition-colors"><Trash2 className="w-4 h-4" /></button>
                <button onClick={() => handleToggle(r)} disabled={toggling === r.id} className="px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-300 hover:text-white hover:bg-slate-700/30 disabled:opacity-50 transition-colors">
                  {toggling === r.id ? <Loader2 className="w-4 h-4 animate-spin" /> : r.enabled ? 'Disable' : 'Enable'}
                </button>
              </div>
            );
          })}
        </div>
      )}
      {tab === 'history' && (
        <div className="space-y-3">
          {!loading && history.length === 0 && <div className="text-center py-12 text-slate-400">No alert history found</div>}
          {history.map((a) => (
            <div key={a.id} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 card-glow transition-all hover:scale-[1.01]">
              <div className="flex items-center gap-2 mb-2">
                <span className={`px-2 py-0.5 rounded-full text-xs border ${SEV_BADGE[a.severity] ?? ''}`}>{a.severity}</span>
                <span className={`px-2 py-0.5 rounded-full text-xs border ${STATUS_BADGE[a.status] ?? ''}`}>{a.status}</span>
                {a.silenced && <span className="flex items-center gap-1 px-2 py-0.5 rounded border border-slate-700/50 text-xs text-slate-400"><VolumeX className="w-3 h-3" /> silenced</span>}
                <span className="font-medium text-white">{a.rule_name}</span>
                <span className="ml-auto text-xs text-slate-400">{new Date(a.fired_at ?? a.timestamp).toLocaleString()}</span>
              </div>
              <div className="text-sm text-slate-400">{a.message}</div>
              {a.resolved_at && <div className="text-xs text-green-400 mt-1">Resolved: {new Date(a.resolved_at).toLocaleString()}</div>}
            </div>
          ))}
        </div>
      )}
      {tab === 'channels' && <ChannelsPanel channels={channels} forbidden={forbidden} onChanged={manualRefresh} onMessage={onMessage} />}
      {tab === 'silences' && <SilencesPanel silences={activeSilences} rules={rules} forbidden={forbidden} onChanged={manualRefresh} onMessage={onMessage} />}
    </div>
  );
};

export default Alerts;
