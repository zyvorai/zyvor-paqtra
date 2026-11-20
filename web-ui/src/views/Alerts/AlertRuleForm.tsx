// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { createAlertRule, updateAlertRule, apiErrorMessage, AlertRule } from '../../services/api';

const inputCls = 'px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500';
const SEVERITIES = ['info', 'warning', 'high', 'critical'];
const CONDITION_HINTS = [
  'drop_rate > 5% for 5m',
  'dns_servfail > 10/min',
  'policy_denied > 100/min',
  'endpoint_status != ready',
];

interface Props {
  /** The rule being edited; omit to create a new one. */
  rule?: AlertRule;
  onSaved: (message: string) => void;
  onCancel: () => void;
}

const AlertRuleForm: React.FC<Props> = ({ rule, onSaved, onCancel }) => {
  const [name, setName] = useState(rule?.name ?? '');
  const [condition, setCondition] = useState(rule?.condition ?? '');
  const [severity, setSeverity] = useState(rule?.severity ?? 'warning');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    const body = { name: name.trim(), condition: condition.trim(), severity };
    try {
      if (rule) await updateAlertRule(rule.id, body);
      else await createAlertRule(body);
      onSaved(rule ? `${body.name} updated` : `${body.name} created`);
    } catch (err) {
      setError(apiErrorMessage(err, 'Failed to save rule'));
      setSaving(false);
    }
  };

  return (
    <div className="mb-4 rounded-xl border border-slate-700/50 bg-slate-800/50 p-4">
      <div className="flex flex-wrap items-end gap-3">
        <label className="text-xs text-slate-400 flex flex-col gap-1">Name
          <input className={inputCls} value={name} onChange={(e) => setName(e.target.value)} placeholder="High drop rate" />
        </label>
        <label className="text-xs text-slate-400 flex flex-col gap-1 flex-1 min-w-[16rem]">Condition
          <input className={`${inputCls} font-mono`} value={condition} onChange={(e) => setCondition(e.target.value)} list="alert-condition-hints" placeholder="drop_rate > 5% for 5m" />
          <datalist id="alert-condition-hints">{CONDITION_HINTS.map((h) => <option key={h} value={h} />)}</datalist>
        </label>
        <label className="text-xs text-slate-400 flex flex-col gap-1">Severity
          <select className={inputCls} value={severity} onChange={(e) => setSeverity(e.target.value)}>
            {SEVERITIES.map((s) => <option key={s} value={s}>{s}</option>)}
          </select>
        </label>
        <button onClick={handleSave} disabled={saving || !name.trim() || !condition.trim()} className="flex items-center gap-2 px-4 py-2 rounded-lg bg-blue-600 text-white text-sm hover:bg-blue-700 disabled:opacity-50 transition-colors">
          {saving && <Loader2 className="w-4 h-4 animate-spin" />}{rule ? 'Save' : 'Create'}
        </button>
        <button onClick={onCancel} className="px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-300 hover:text-white hover:bg-slate-700/30 transition-colors">Cancel</button>
      </div>
      {error && <div className="mt-3 text-sm text-red-400">{error}</div>}
    </div>
  );
};

export default AlertRuleForm;
