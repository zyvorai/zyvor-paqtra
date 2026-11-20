// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useCallback, useEffect, useState } from 'react';
import axios from 'axios';
import { useSearchParams } from 'react-router-dom';
import { ListChecks, Loader2, Pencil, Plus, Trash2, FlaskConical } from 'lucide-react';
import {
  fetchPolicies, fetchPolicy, addPolicyRule, updatePolicyRule, deletePolicyRule, simulatePolicy,
  apiErrorMessage, RULE_DIRECTIONS,
  Policy, PolicyDetail, PolicyRule, RuleChangeResult, RuleDirection,
} from '../../services/api';
import { usePageTitle } from '../../hooks/usePageTitle';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';

const DIRECTION_LABEL: Record<RuleDirection, string> = {
  ingress: 'Ingress (allow)', egress: 'Egress (allow)', ingressDeny: 'Ingress deny', egressDeny: 'Egress deny',
};
const KUBE_DNS = { matchLabels: { 'k8s:io.kubernetes.pod.namespace': 'kube-system', 'k8s-app': 'kube-dns' } };
const HTTPS = { ports: [{ port: '443', protocol: 'TCP' }] };

/** Starting points per direction. The first entry is what "Add rule" opens with. */
const TEMPLATES: Record<RuleDirection, { label: string; rule: PolicyRule }[]> = {
  ingress: [
    { label: 'From pods by label, on a port', rule: { fromEndpoints: [{ matchLabels: { app: 'frontend' } }], toPorts: [{ ports: [{ port: '8080', protocol: 'TCP' }] }] } },
    { label: 'From entities (cluster, host, world, ...)', rule: { fromEntities: ['cluster'] } },
    { label: 'From a CIDR with exceptions', rule: { fromCIDRSet: [{ cidr: '10.0.0.0/8', except: ['10.96.0.0/12'] }] } },
    { label: 'ICMP echo request', rule: { fromEndpoints: [{}], icmps: [{ fields: [{ type: 8, family: 'IPv4' }] }] } },
    { label: 'L7 HTTP: GET /healthz only', rule: { fromEndpoints: [{ matchLabels: { app: 'frontend' } }], toPorts: [{ ports: [{ port: '8080', protocol: 'TCP' }], rules: { http: [{ method: 'GET', path: '/healthz' }] } }] } },
  ],
  egress: [
    { label: 'To kube-dns, with DNS visibility', rule: { toEndpoints: [KUBE_DNS], toPorts: [{ ports: [{ port: '53', protocol: 'ANY' }], rules: { dns: [{ matchPattern: '*' }] } }] } },
    { label: 'To an FQDN over HTTPS', rule: { toFQDNs: [{ matchName: 'api.example.com' }], toPorts: [HTTPS] } },
    { label: 'To entities (kube-apiserver, world, ...)', rule: { toEntities: ['kube-apiserver'] } },
    { label: 'To pods by label', rule: { toEndpoints: [{ matchLabels: { app: 'db' } }], toPorts: [{ ports: [{ port: '5432', protocol: 'TCP' }] }] } },
    { label: 'To a CIDR with exceptions', rule: { toCIDRSet: [{ cidr: '10.0.0.0/8', except: ['10.96.0.0/12'] }] } },
  ],
  ingressDeny: [
    { label: 'Deny from a CIDR', rule: { fromCIDR: ['203.0.113.0/24'] } },
    { label: 'Deny from entities', rule: { fromEntities: ['world'] } },
  ],
  egressDeny: [
    { label: 'Deny to a CIDR', rule: { toCIDR: ['203.0.113.0/24'] } },
    { label: 'Deny to entities', rule: { toEntities: ['world'] } },
  ],
};

const btnCls = 'flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-300 hover:text-white hover:bg-slate-700/30 disabled:opacity-50 transition-colors';
const primaryCls = 'flex items-center gap-2 px-4 py-2 rounded-lg bg-blue-600 text-white text-sm hover:bg-blue-700 disabled:opacity-50 transition-colors';
const inputCls = 'px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500';

/** What the JSON editor is doing: adding to a direction, or replacing rule `index`. */
interface Draft { direction: RuleDirection; index: number | null; text: string }

const pretty = (v: unknown) => JSON.stringify(v, null, 2);
const label = (p: { namespace: string; name: string }) => `${p.namespace}/${p.name}`;

const PolicyRules: React.FC = () => {
  usePageTitle('Policy Rules');
  const [params] = useSearchParams();
  const [policies, setPolicies] = useState<Policy[]>([]);
  const [selected, setSelected] = useState('');
  const [detail, setDetail] = useState<PolicyDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<{ change: RuleChangeResult; simulation: unknown } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);

  const load = useCallback(async (id: string) => {
    if (!id) { setDetail(null); return; }
    setLoading(true); setError(null);
    try { setDetail((await fetchPolicy(id)).data); }
    catch (e) { setDetail(null); setError(apiErrorMessage(e, 'Failed to load policy')); }
    finally { setLoading(false); }
  }, []);

  useEffect(() => {
    fetchPolicies().then((r) => setPolicies(r.data.policies ?? [])).catch((e) => setError(apiErrorMessage(e, 'Failed to list policies')));
    // Deep link from the Policies page: /policy-rules?policy=<id>
    const wanted = params.get('policy');
    if (wanted) { setSelected(wanted); void load(wanted); }
  }, [params, load]);

  const select = (id: string) => { setSelected(id); setDraft(null); setPreview(null); load(id); };

  const parseDraft = (): PolicyRule | null => {
    if (!draft) return null;
    try {
      const rule = JSON.parse(draft.text);
      if (rule === null || typeof rule !== 'object' || Array.isArray(rule)) throw new Error('rule must be a JSON object');
      return rule as PolicyRule;
    } catch (e) { setError(`Invalid rule JSON: ${e instanceof Error ? e.message : e}`); return null; }
  };

  const send = (rule: PolicyRule, dryRun: boolean) => {
    if (!draft || !detail) throw new Error('nothing to send');
    const rv = detail.resource_version;
    return draft.index === null
      ? addPolicyRule(selected, { direction: draft.direction, rule, resource_version: rv }, dryRun)
      : updatePolicyRule(selected, { direction: draft.direction, index: draft.index, rule, resource_version: rv }, dryRun);
  };

  // A 409 means someone changed the policy since we read it: show the fresh copy.
  const fail = async (e: unknown, fallback: string) => {
    // Reload first: load() clears the error, and the conflict message must survive it.
    if (axios.isAxiosError(e) && e.response?.status === 409) await load(selected);
    setError(apiErrorMessage(e, fallback));
  };

  const handlePreview = async () => {
    const rule = parseDraft(); if (!rule) return;
    setBusy(true); setError(null); setPreview(null);
    try {
      // Server-side dry run validates the result against the cluster; simulate shows impact.
      const change = (await send(rule, true)).data;
      const simulation = (await simulatePolicy({ name: detail!.name, namespace: detail!.namespace, spec: change.spec })).data;
      setPreview({ change, simulation });
    } catch (e) { await fail(e, 'Preview failed'); }
    finally { setBusy(false); }
  };

  const handleApply = async () => {
    const rule = parseDraft(); if (!rule) return;
    setBusy(true); setError(null);
    try {
      await send(rule, false);
      setSuccess(draft!.index === null ? 'Rule added' : 'Rule updated');
      setDraft(null); setPreview(null);
      await load(selected);
    } catch (e) { await fail(e, 'Failed to save rule'); }
    finally { setBusy(false); }
  };

  const handleDelete = async (direction: RuleDirection, index: number) => {
    if (!detail || !window.confirm(`Delete ${direction} rule #${index + 1} from ${label(detail)}?`)) return;
    setBusy(true); setError(null);
    try {
      await deletePolicyRule(selected, { direction, index, resource_version: detail.resource_version });
      setSuccess('Rule deleted');
      await load(selected);
    } catch (e) { await fail(e, 'Failed to delete rule'); }
    finally { setBusy(false); }
  };

  const openDraft = (direction: RuleDirection, index: number | null, rule?: PolicyRule) => {
    setPreview(null); setError(null);
    setDraft({ direction, index, text: pretty(rule ?? TEMPLATES[direction][0].rule) });
  };

  const rulesOf = (d: RuleDirection): PolicyRule[] => {
    const v = detail?.spec?.[d];
    return Array.isArray(v) ? (v as PolicyRule[]) : [];
  };

  const tick = useChanged(policies);
  const ruleTotal = RULE_DIRECTIONS.reduce((n, d) => n + rulesOf(d).length, 0);

  return (
    <div className="netra-page">
      <div className="page-chrome mb-6">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20"><ListChecks className="w-5 h-5 text-white" /></div>
          <h1 className="text-2xl font-bold text-white">Policy Rules</h1>
        </div>
        <p className="text-sm text-slate-400 mt-1">Add, edit and delete individual rules of a CiliumNetworkPolicy. Changes are applied through the Cilium CRD.</p>
      </div>
      <PagePulse
        tick={tick}
        live={false}
        headline={tick ? (detail ? `${label(detail)} has ${ruleTotal} rule${ruleTotal === 1 ? '' : 's'}.` : `${policies.length} policies — pick one to edit its rules.`) : undefined}
        figures={[
          { label: 'policies', value: tick ? policies.length : undefined },
          ...RULE_DIRECTIONS.map((d) => ({ label: `${d} rules`, value: detail ? rulesOf(d).length : undefined })),
        ]}
      />
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      <label className="text-xs text-slate-400 flex flex-col gap-1 w-fit mb-4">Policy
        <select className={inputCls} value={selected} onChange={(e) => select(e.target.value)} aria-label="Policy">
          <option value="">Select a policy…</option>
          {policies.map((p) => <option key={p.id || label(p)} value={p.id || label(p)}>{label(p)}</option>)}
        </select>
      </label>

      {loading && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}
      {detail && !loading && (
        <div className="space-y-6">
          {RULE_DIRECTIONS.map((d) => (
            <section key={d}>
              <div className="flex items-center justify-between mb-2">
                <h2 className="text-sm font-semibold text-white">{DIRECTION_LABEL[d]} <span className="text-slate-400 font-normal">({rulesOf(d).length})</span></h2>
                <button className={btnCls} onClick={() => openDraft(d, null)} disabled={busy}><Plus className="w-4 h-4" /> Add {d} rule</button>
              </div>
              {rulesOf(d).length === 0 && <div className="text-sm text-slate-500 py-2">No {d} rules</div>}
              <div className="space-y-2">
                {rulesOf(d).map((rule, i) => (
                  <div key={i} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-3 flex items-start gap-3">
                    <span className="text-xs text-slate-400 w-6 pt-1">#{i + 1}</span>
                    <pre className="flex-1 text-xs text-slate-300 font-mono overflow-x-auto">{pretty(rule)}</pre>
                    <button className={btnCls} aria-label={`Edit ${d} rule ${i + 1}`} onClick={() => openDraft(d, i, rule)} disabled={busy}><Pencil className="w-4 h-4" /></button>
                    <button className={btnCls} aria-label={`Delete ${d} rule ${i + 1}`} onClick={() => handleDelete(d, i)} disabled={busy}><Trash2 className="w-4 h-4" /></button>
                  </div>
                ))}
              </div>
            </section>
          ))}
        </div>
      )}

      {draft && detail && (
        <div className="mt-6 rounded-xl border border-blue-500/30 bg-slate-800/60 p-4">
          <div className="text-sm text-white mb-2">{draft.index === null ? `New ${draft.direction} rule` : `Edit ${draft.direction} rule #${draft.index + 1}`} <span className="text-slate-400">in {label(detail)}</span></div>
          {draft.index === null && (
            <label className="text-xs text-slate-400 flex flex-col gap-1 w-fit mb-2">Start from
              <select className={inputCls} aria-label="Template" defaultValue="0"
                onChange={(e) => { setDraft({ ...draft, text: pretty(TEMPLATES[draft.direction][Number(e.target.value)].rule) }); setPreview(null); }}>
                {TEMPLATES[draft.direction].map((t, i) => <option key={t.label} value={i}>{t.label}</option>)}
              </select>
            </label>
          )}
          <textarea aria-label="Rule JSON" className={`${inputCls} w-full font-mono text-xs h-48`} value={draft.text} spellCheck={false}
            onChange={(e) => { setDraft({ ...draft, text: e.target.value }); setPreview(null); }} />
          <div className="flex gap-2 mt-3">
            <button className={btnCls} onClick={handlePreview} disabled={busy}>{busy ? <Loader2 className="w-4 h-4 animate-spin" /> : <FlaskConical className="w-4 h-4" />} Preview</button>
            <button className={primaryCls} onClick={handleApply} disabled={busy}>Apply</button>
            <button className={btnCls} onClick={() => { setDraft(null); setPreview(null); }} disabled={busy}>Cancel</button>
          </div>
          {preview && (
            <div className="mt-3 text-xs text-slate-300">
              <div className="text-green-400 mb-1">Dry run passed: the cluster accepts the resulting policy ({Object.entries(preview.change.rule_counts).map(([k, v]) => `${k}: ${v}`).join(', ')}).</div>
              <div className="text-slate-400 mb-1">Simulated impact (unsupported constructs are reported as unknown):</div>
              <pre className="font-mono overflow-x-auto max-h-64 bg-slate-900/50 rounded p-2">{pretty(preview.simulation)}</pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
};

export default PolicyRules;
