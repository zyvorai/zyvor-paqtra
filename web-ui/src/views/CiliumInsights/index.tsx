// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useCallback, useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { Sparkles, Loader2 } from 'lucide-react';
import {
  fetchCiliumFeatures, fetchHubbleNodes, fetchHubbleMetrics, fetchCiliumMetrics, fetchCiliumResources,
  fetchAgentQuery, apiErrorMessage, RESOURCE_KINDS,
  CiliumFeature, HubbleNodeInfo, MetricsReport, CiliumResource, FeatureState,
} from '../../services/api';
import { usePageTitle } from '../../hooks/usePageTitle';
import PagePulse from '../../components/kit/PagePulse';

type Tab = 'features' | 'hubble' | 'metrics' | 'resources' | 'agent';

const inputCls = 'px-3 py-2 rounded-lg bg-slate-900/50 border border-slate-700/50 text-sm text-white focus:outline-none focus:ring-2 focus:ring-blue-500';
const btnCls = 'flex items-center gap-2 px-3 py-2 rounded-lg border border-slate-700/50 text-sm text-slate-300 hover:text-white hover:bg-slate-700/30 disabled:opacity-50 transition-colors';
const STATE_CLS: Record<FeatureState, string> = {
  enabled: 'bg-green-500/15 text-green-400 border-green-500/30',
  disabled: 'bg-slate-500/15 text-slate-400 border-slate-500/30',
  set: 'bg-blue-500/15 text-blue-400 border-blue-500/30',
  unknown: 'bg-yellow-500/10 text-yellow-400 border-yellow-500/30',
};
const AGENT_QUERIES = ['endpoints', 'identities', 'services', 'fqdn-cache', 'status', 'policy-selectors'];

const Err: React.FC<{ msg: string | null }> = ({ msg }) =>
  msg ? <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{msg}</div> : null;

/* ---------------------------------------------------------------- Features */
const FeaturesTab: React.FC = () => {
  const [features, setFeatures] = useState<CiliumFeature[] | null>(null);
  const [reason, setReason] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    fetchCiliumFeatures()
      .then((r) => { setFeatures(r.data.features); setReason(r.data.available ? null : r.data.reason ?? 'unavailable'); })
      .catch((e) => setError(apiErrorMessage(e, 'Failed to read Cilium features')));
  }, []);
  if (error) return <Err msg={error} />;
  if (!features) return <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />;
  if (reason) return <div className="text-slate-400 py-8 text-center">{reason}</div>;
  const categories = Array.from(new Set(features.map((f) => f.category)));
  const count = (st: FeatureState) => features.filter((f) => f.state === st).length;
  return (
    <div className="space-y-6">
      <PagePulse
        headline={`${count('enabled')} of ${features.length} Cilium features enabled.`}
        tick={features}
        live={false}
        figures={[
          { label: 'features on', value: count('enabled') },
          { label: 'keys set', value: count('set') },
          { label: 'features off', value: count('disabled') },
          { label: 'keys absent', value: count('unknown'), tone: count('unknown') ? 'warn' : undefined },
          { label: 'categories', value: categories.length },
        ]}
      />
      <p className="text-sm text-slate-400">From the <code>cilium-config</code> ConfigMap. <span className="text-yellow-400">unknown</span> means the key is absent (an older Cilium, or never configured), not that the feature is off.</p>
      {categories.map((c) => (
        <section key={c}>
          <h2 className="text-sm font-semibold text-white mb-2">{c}</h2>
          <div className="grid gap-2 md:grid-cols-2">
            {features.filter((f) => f.category === c).map((f) => (
              <div key={f.key} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-3 flex items-center gap-3">
                <span className={`px-2 py-0.5 rounded-full text-xs border ${STATE_CLS[f.state]}`}>{f.state}</span>
                <div className="flex-1 min-w-0">
                  <div className="text-sm text-white">{f.title}</div>
                  {f.value && <div className="text-xs text-slate-400 font-mono truncate" title={f.value}>{f.value}</div>}
                </div>
                {f.view && f.state !== 'unknown' && <Link className="text-xs text-blue-400 hover:underline" to={f.view}>Open</Link>}
              </div>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
};

/* ------------------------------------------------------------------ Hubble */
const HubbleTab: React.FC = () => {
  const [nodes, setNodes] = useState<HubbleNodeInfo[] | null>(null);
  const [errors, setErrors] = useState<{ cluster: string; error: string }[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    fetchHubbleNodes()
      .then((r) => { setNodes(r.data.nodes); setErrors(r.data.errors); })
      .catch((e) => setError(apiErrorMessage(e, 'Failed to read Hubble nodes')));
  }, []);
  if (error) return <Err msg={error} />;
  if (!nodes) return <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />;
  return (
    <div>
      {errors.map((e) => <Err key={e.cluster} msg={`${e.cluster}: ${e.error}`} />)}
      {nodes.length === 0 && errors.length === 0 && <div className="text-slate-400 py-8 text-center">Hubble reported no nodes</div>}
      <div className="space-y-2">
        {nodes.map((n) => {
          const fill = n.max_flows > 0 ? Math.round((n.num_flows / n.max_flows) * 100) : 0;
          return (
            <div key={`${n.cluster}/${n.name}`} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-3">
              <div className="flex items-center gap-3 flex-wrap">
                <span className="text-white font-medium">{n.name}</span>
                {n.state && <span className={`px-2 py-0.5 rounded-full text-xs border ${n.state === 'NODE_CONNECTED' ? STATE_CLS.enabled : STATE_CLS.unknown}`}>{n.state.replace('NODE_', '').toLowerCase()}</span>}
                <span className="text-xs text-slate-400">{n.version}</span>
                <span className="text-xs text-slate-400 ml-auto">up {Math.floor(n.uptime_seconds / 3600)}h</span>
              </div>
              <div className="mt-2 text-xs text-slate-400">
                Flow buffer {n.num_flows.toLocaleString()} / {n.max_flows.toLocaleString()} ({fill}%), {n.seen_flows.toLocaleString()} seen since start
                {fill >= 100 && <span className="text-yellow-400"> — buffer is full, older flows are being overwritten</span>}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
};

/* ----------------------------------------------------------------- Metrics */
const MetricList: React.FC<{ title: string; report: MetricsReport | null }> = ({ title, report }) => {
  if (!report) return <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-4" />;
  return (
    <section>
      <h2 className="text-sm font-semibold text-white mb-2">{title}</h2>
      {!report.available && report.reason && <div className="text-sm text-slate-400 mb-2">{report.reason}. Set <code>PROMETHEUS_URL</code> to a Prometheus that scrapes Cilium.</div>}
      <div className="grid gap-2 md:grid-cols-2">
        {report.metrics.map((m) => (
          <div key={m.key} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-3">
            <div className="text-sm text-white">{m.title} <span className="text-xs text-slate-400">({m.unit})</span></div>
            {m.series.length === 0 && <div className="text-xs text-slate-500 mt-1">{m.error ?? `No data. ${m.hint ?? ''}`}</div>}
            {m.series.slice(0, 5).map((s, i) => (
              <div key={i} className="flex justify-between text-xs mt-1 gap-2">
                <span className="text-slate-300 truncate" title={JSON.stringify(s.labels)}>{Object.values(s.labels).join(' · ') || 'total'}</span>
                <span className="text-slate-400 font-mono">{s.value.toPrecision(3)}</span>
              </div>
            ))}
          </div>
        ))}
      </div>
    </section>
  );
};

const MetricsTab: React.FC = () => {
  const [hubble, setHubble] = useState<MetricsReport | null>(null);
  const [cilium, setCilium] = useState<MetricsReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    fetchHubbleMetrics().then((r) => setHubble(r.data)).catch((e) => setError(apiErrorMessage(e, 'Failed to read Hubble metrics')));
    fetchCiliumMetrics().then((r) => setCilium(r.data)).catch((e) => setError(apiErrorMessage(e, 'Failed to read Cilium metrics')));
  }, []);
  return (
    <div className="space-y-6">
      <Err msg={error} />
      <MetricList title="Hubble" report={hubble} />
      <MetricList title="Cilium agent" report={cilium} />
    </div>
  );
};

/* --------------------------------------------------------------- Resources */
const ResourcesTab: React.FC = () => {
  const [kind, setKind] = useState(RESOURCE_KINDS[0].kind);
  const [items, setItems] = useState<CiliumResource[] | null>(null);
  const [installed, setInstalled] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(async (k: string) => {
    setItems(null); setError(null);
    try { const r = await fetchCiliumResources(k); setItems(r.data.items); setInstalled(r.data.installed); }
    catch (e) { setError(apiErrorMessage(e, 'Failed to list resources')); }
  }, []);
  useEffect(() => { void load(kind); }, [kind, load]);
  return (
    <div>
      <select className={`${inputCls} mb-4`} aria-label="Resource kind" value={kind} onChange={(e) => setKind(e.target.value)}>
        {RESOURCE_KINDS.map((k) => <option key={k.kind} value={k.kind}>{k.label}</option>)}
      </select>
      <Err msg={error} />
      {!items && !error && <Loader2 className="w-6 h-6 animate-spin text-blue-400 mx-auto my-8" />}
      {items && !installed && <div className="text-slate-400 py-6 text-center">This CRD is not installed (or not readable) in the cluster.</div>}
      {items && installed && items.length === 0 && <div className="text-slate-400 py-6 text-center">None found</div>}
      <div className="space-y-2">
        {items?.map((it) => (
          <details key={`${it.namespace}/${it.name}`} className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-3">
            <summary className="cursor-pointer text-sm text-white">{it.namespace ? `${it.namespace}/` : ''}{it.name}</summary>
            <pre className="mt-2 text-xs text-slate-300 font-mono overflow-x-auto">{JSON.stringify({ spec: it.spec, status: it.status }, null, 2)}</pre>
          </details>
        ))}
      </div>
    </div>
  );
};

/* ------------------------------------------------------------ Agent + trace */
const AgentTab: React.FC = () => {
  const [what, setWhat] = useState(AGENT_QUERIES[0]);
  const [out, setOut] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const runQuery = async () => {
    setBusy(true); setError(null); setOut(null);
    try { setOut(JSON.stringify((await fetchAgentQuery(what)).data.data, null, 2)); }
    catch (e) { setError(apiErrorMessage(e, 'Query failed')); }
    finally { setBusy(false); }
  };
  return (
    <div>
      <Err msg={error} />
      <p className="text-xs text-slate-400 mb-3">Read-only output from one Cilium agent (a sample of one node, not the whole cluster). <code>policy-selectors</code> shows which policies select which identities.</p>
      <div className="flex gap-2 mb-3">
        <select className={inputCls} aria-label="Agent query" value={what} onChange={(e) => setWhat(e.target.value)}>
          {AGENT_QUERIES.map((q) => <option key={q} value={q}>{q}</option>)}
        </select>
        <button className={btnCls} onClick={runQuery} disabled={busy}>Run</button>
      </div>
      {out && <pre className="text-xs text-slate-300 font-mono overflow-auto max-h-96 bg-slate-900/50 rounded p-2">{out}</pre>}
    </div>
  );
};

/* -------------------------------------------------------------------- Page */
const TABS: { id: Tab; label: string }[] = [
  { id: 'features', label: 'Features' }, { id: 'hubble', label: 'Hubble' }, { id: 'metrics', label: 'Metrics' },
  { id: 'resources', label: 'Resources' }, { id: 'agent', label: 'Agent' },
];

const CiliumInsights: React.FC = () => {
  usePageTitle('Cilium Insights');
  const [tab, setTab] = useState<Tab>('features');
  return (
    <div className="netra-page">
      <div className="page-chrome mb-6">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-cyan-500 to-cyan-700 flex items-center justify-center shadow-lg shadow-cyan-500/20"><Sparkles className="w-5 h-5 text-white" /></div>
          <h1 className="text-2xl font-bold text-white">Cilium Insights</h1>
        </div>
        <p className="text-sm text-slate-400 mt-1">Which Cilium features are on, how Hubble is doing, and read-only views into the agent. Nothing here changes Cilium.</p>
      </div>
      <div className="flex gap-1 mb-4 p-1 rounded-lg bg-slate-900/50 w-fit">
        {TABS.map((t) => (
          <button key={t.id} onClick={() => setTab(t.id)} className={`px-4 py-2 rounded-md text-sm transition-colors ${tab === t.id ? 'bg-slate-800/50 text-white shadow' : 'text-slate-400'}`}>{t.label}</button>
        ))}
      </div>
      {tab === 'features' && <FeaturesTab />}
      {tab === 'hubble' && <HubbleTab />}
      {tab === 'metrics' && <MetricsTab />}
      {tab === 'resources' && <ResourcesTab />}
      {tab === 'agent' && <AgentTab />}
    </div>
  );
};

export default CiliumInsights;
