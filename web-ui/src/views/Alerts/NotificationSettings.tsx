// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState } from 'react';
import { Loader2, Plus, Send, Trash2, VolumeX } from 'lucide-react';
import {
  createChannel, deleteChannel, testChannel, createSilence, deleteSilence, apiErrorMessage,
  AlertRule, AlertSilence, ChannelKind, NotificationChannel,
} from '../../services/api';

const inputCls = 'px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500';
const btnCls = 'flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-300 hover:text-white hover:bg-slate-700/30 disabled:opacity-50 transition-colors';
const primaryCls = 'flex items-center gap-2 px-4 py-2 rounded-lg bg-blue-600 text-white text-sm hover:bg-blue-700 disabled:opacity-50 transition-colors';

const KIND_LABEL: Record<ChannelKind, string> = { webhook: 'Webhook', slack: 'Slack', pagerduty: 'PagerDuty' };
const TARGET_LABEL: Record<ChannelKind, string> = { webhook: 'Webhook URL', slack: 'Slack incoming webhook URL', pagerduty: 'Events v2 routing key' };
const SEVERITIES = ['info', 'warning', 'high', 'critical'];
const DURATIONS: { label: string; minutes: number }[] = [
  { label: '30 minutes', minutes: 30 }, { label: '1 hour', minutes: 60 }, { label: '4 hours', minutes: 240 },
  { label: '24 hours', minutes: 1440 }, { label: '7 days', minutes: 10080 },
];

export interface PanelProps {
  /** Set when the API refused the request because the user is not an admin. */
  forbidden: boolean;
  onChanged: () => void;
  onMessage: (kind: 'success' | 'error', text: string) => void;
}

const Forbidden: React.FC = () => (
  <div className="text-center py-12 text-slate-400">Admin role required to manage notification settings.</div>
);

export const ChannelsPanel: React.FC<PanelProps & { channels: NotificationChannel[] }> = ({ channels, forbidden, onChanged, onMessage }) => {
  const [showForm, setShowForm] = useState(false);
  const [name, setName] = useState('');
  const [kind, setKind] = useState<ChannelKind>('slack');
  const [target, setTarget] = useState('');
  const [minSeverity, setMinSeverity] = useState('');
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState<string | null>(null);

  if (forbidden) return <Forbidden />;

  const handleCreate = async () => {
    setSaving(true);
    try {
      await createChannel({ name: name.trim(), kind, target: target.trim(), min_severity: minSeverity || undefined });
      onMessage('success', 'Channel added');
      setShowForm(false); setName(''); setTarget(''); setMinSeverity('');
      onChanged();
    } catch (err) { onMessage('error', apiErrorMessage(err, 'Failed to add channel')); }
    finally { setSaving(false); }
  };

  const handleTest = async (c: NotificationChannel) => {
    setTesting(c.id);
    try {
      const { data } = await testChannel(c.id);
      if (data.delivered) onMessage('success', `Test notification delivered to ${c.name}`);
      else onMessage('error', `Test to ${c.name} failed after ${data.attempts} attempt(s): ${data.error ?? 'unknown error'}`);
    } catch (err) { onMessage('error', apiErrorMessage(err, 'Test failed')); }
    finally { setTesting(null); }
  };

  const handleDelete = async (c: NotificationChannel) => {
    if (!window.confirm(`Delete channel "${c.name}"? Alerts will no longer be sent to it.`)) return;
    try { await deleteChannel(c.id); onMessage('success', 'Channel deleted'); onChanged(); }
    catch (err) { onMessage('error', apiErrorMessage(err, 'Failed to delete channel')); }
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <p className="text-sm text-slate-400">Alerts go to every enabled channel whose minimum severity they meet. The destination is masked once saved.</p>
        <button onClick={() => setShowForm((v) => !v)} className={btnCls}><Plus className="w-4 h-4" /> Add channel</button>
      </div>

      {showForm && (
        <div className="mb-4 rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 flex flex-wrap items-end gap-3">
          <label className="text-xs text-slate-400 flex flex-col gap-1">Name<input className={inputCls} value={name} onChange={(e) => setName(e.target.value)} placeholder="on-call" /></label>
          <label className="text-xs text-slate-400 flex flex-col gap-1">Type
            <select className={inputCls} value={kind} onChange={(e) => setKind(e.target.value as ChannelKind)}>
              {(Object.keys(KIND_LABEL) as ChannelKind[]).map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
            </select>
          </label>
          <label className="text-xs text-slate-400 flex flex-col gap-1 flex-1 min-w-[16rem]">{TARGET_LABEL[kind]}
            <input className={inputCls} type="password" autoComplete="off" value={target} onChange={(e) => setTarget(e.target.value)} placeholder={kind === 'pagerduty' ? 'routing key' : 'https://…'} />
          </label>
          <label className="text-xs text-slate-400 flex flex-col gap-1">Minimum severity
            <select className={inputCls} value={minSeverity} onChange={(e) => setMinSeverity(e.target.value)}>
              <option value="">Any</option>
              {SEVERITIES.map((s) => <option key={s} value={s}>{s}</option>)}
            </select>
          </label>
          <button onClick={handleCreate} disabled={saving || !name.trim() || !target.trim()} className={primaryCls}>{saving && <Loader2 className="w-4 h-4 animate-spin" />}Add</button>
        </div>
      )}

      {channels.length === 0 && (
        <div className="text-center py-12 text-slate-400">No notification channels. Alerts are recorded in History but not sent anywhere.</div>
      )}
      <div className="space-y-3">
        {channels.map((c) => (
          <div key={c.id} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 flex items-center gap-4">
            <div className="flex-1">
              <div className="flex items-center gap-2 mb-1">
                <span className="font-medium text-white">{c.name}</span>
                <span className="px-2 py-0.5 rounded border border-slate-700/50 text-xs text-slate-300">{KIND_LABEL[c.kind] ?? c.kind}</span>
                {c.min_severity && <span className="px-2 py-0.5 rounded-full text-xs border border-slate-700/50 text-slate-400">≥ {c.min_severity}</span>}
                {!c.enabled && <span className="px-2 py-0.5 rounded border border-slate-700/50 text-xs text-slate-400">disabled</span>}
              </div>
              <div className="text-sm text-slate-400 font-mono">{c.target}</div>
            </div>
            <button onClick={() => handleTest(c)} disabled={testing === c.id} className={btnCls} title="Send a test notification">
              {testing === c.id ? <Loader2 className="w-4 h-4 animate-spin" /> : <Send className="w-4 h-4" />} Test
            </button>
            <button onClick={() => handleDelete(c)} className="p-2 rounded-lg text-slate-400 hover:text-red-400 hover:bg-slate-700/30 transition-colors" title="Delete channel"><Trash2 className="w-4 h-4" /></button>
          </div>
        ))}
      </div>
    </div>
  );
};

export const SilencesPanel: React.FC<PanelProps & { silences: AlertSilence[]; rules: AlertRule[] }> = ({ silences, rules, forbidden, onChanged, onMessage }) => {
  const [showForm, setShowForm] = useState(false);
  const [ruleId, setRuleId] = useState('');
  const [minutes, setMinutes] = useState(60);
  const [comment, setComment] = useState('');
  const [saving, setSaving] = useState(false);

  if (forbidden) return <Forbidden />;

  const ruleName = (id: string | null) => (id ? rules.find((r) => r.id === id)?.name ?? id : 'All rules');

  const handleCreate = async () => {
    setSaving(true);
    try {
      await createSilence({ rule_id: ruleId || undefined, duration_minutes: minutes, comment: comment.trim() || undefined });
      onMessage('success', 'Silence created'); setShowForm(false); setComment(''); onChanged();
    } catch (err) { onMessage('error', apiErrorMessage(err, 'Failed to create silence')); }
    finally { setSaving(false); }
  };

  const handleDelete = async (s: AlertSilence) => {
    try { await deleteSilence(s.id); onMessage('success', 'Silence removed'); onChanged(); }
    catch (err) { onMessage('error', apiErrorMessage(err, 'Failed to remove silence')); }
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-4">
        <p className="text-sm text-slate-400">While a silence is active, matching alerts are recorded in History but send no notification and open no incident.</p>
        <button onClick={() => setShowForm((v) => !v)} className={btnCls}><VolumeX className="w-4 h-4" /> New silence</button>
      </div>

      {showForm && (
        <div className="mb-4 rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 flex flex-wrap items-end gap-3">
          <label className="text-xs text-slate-400 flex flex-col gap-1">Rule
            <select className={inputCls} value={ruleId} onChange={(e) => setRuleId(e.target.value)}>
              <option value="">All rules</option>
              {rules.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
            </select>
          </label>
          <label className="text-xs text-slate-400 flex flex-col gap-1">For
            <select className={inputCls} value={minutes} onChange={(e) => setMinutes(Number(e.target.value))}>
              {DURATIONS.map((d) => <option key={d.minutes} value={d.minutes}>{d.label}</option>)}
            </select>
          </label>
          <label className="text-xs text-slate-400 flex flex-col gap-1 flex-1 min-w-[12rem]">Reason (optional)
            <input className={inputCls} value={comment} onChange={(e) => setComment(e.target.value)} placeholder="planned maintenance" />
          </label>
          <button onClick={handleCreate} disabled={saving} className={primaryCls}>{saving && <Loader2 className="w-4 h-4 animate-spin" />}Silence</button>
        </div>
      )}

      {silences.length === 0 && <div className="text-center py-12 text-slate-400">No active silences.</div>}
      <div className="space-y-3">
        {silences.map((s) => (
          <div key={s.id} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 flex items-center gap-4">
            <VolumeX className="w-5 h-5 text-slate-400" />
            <div className="flex-1">
              <div className="font-medium text-white">{ruleName(s.rule_id)}</div>
              <div className="text-xs text-slate-400">
                Until {new Date(s.until).toLocaleString()} · by {s.created_by}{s.comment ? ` · ${s.comment}` : ''}
              </div>
            </div>
            <button onClick={() => handleDelete(s)} className={btnCls}>End silence</button>
          </div>
        ))}
      </div>
    </div>
  );
};
