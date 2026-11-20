// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState, type ReactNode } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { useCountUp } from '../../hooks/useCountUp';
import Reveal from '../../components/Reveal';
import DatapathHero from '../../components/DatapathHero';
import { buildDigest, dropPackets, type Digest } from '../../components/digest';
import { PulseFigure } from '../../components/kit/PagePulse';
import RankedList from '../../components/kit/RankedList';
import ToneDot from '../../components/kit/ToneDot';
import { scoreTone, type Tone } from '../../components/kit/tone';
import { compact } from '../../components/kit/format';
import { useRate, useSeries } from '../../components/kit/useSeries';
import {
  fetchNodes,
  fetchEndpoints,
  fetchCiliumStatus,
  fetchEbpfSummary,
  fetchEbpfDrops,
  fetchClusterHealth,
  fetchFlowStats,
  fetchMetricsSummary,
  fetchDnsStats,
  fetchAnomalies,
} from '../../services/api';

const TILE_TIMEOUT_MS = 8_000;
const STALE_AFTER_MS = 45_000;

function Metric({ value, label }: { value: number | string; label: string }) {
  const numeric = typeof value === 'number' && Number.isFinite(value);
  const animated = useCountUp(numeric ? (value as number) : 0);
  return (
    <div>
      <b>{numeric ? compact(animated) : value}</b>
      <span>{label}</span>
    </div>
  );
}

function StaleBadge({ at }: { at: number | null }) {
  if (at == null) return null;
  // Staleness is measured against the current time on every render.
  // eslint-disable-next-line react-hooks/purity
  const age = Date.now() - at;
  if (age < STALE_AFTER_MS) return null;
  const secs = Math.round(age / 1000);
  return (
    <span
      className="severity-badge warning"
      style={{ fontSize: 11, marginLeft: 8 }}
      title={`Last successful refresh ${secs}s ago`}
    >
      stale {secs}s
    </span>
  );
}

type Settled<T> = { ok: true; value: T } | { ok: false; error: string };

async function settle<T>(p: Promise<{ data: T }>, timeoutMs = TILE_TIMEOUT_MS): Promise<Settled<T>> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const raced = Promise.race([
      p.then((r) => ({ data: r.data })),
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(`tile timeout ${timeoutMs}ms`)), timeoutMs);
      }),
    ]);
    const { data } = await raced;
    return { ok: true, value: data };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  } finally {
    if (timer) clearTimeout(timer);
  }
}

type TileKey = 'cluster' | 'digest' | 'flows' | 'ebpf' | 'platform';

type DropRow = { reason: string; count: number; direction?: string; bytes?: number };
type AnomalyRow = { id?: string; severity?: string; description?: string; anomaly_type?: string; source_pod?: string | null; source_namespace?: string | null; status?: string };
type DnsRow = { total_queries?: number; total?: number; failures?: number; l7_observed?: number; l4_only?: number; avg_latency_ms?: number; source?: string };

const FLOW_POLL_MS = 5_000;
const SLOW_POLL_MS = 15_000;

function sevClass(sev?: string): string {
  const s = (sev || '').toLowerCase();
  if (s === 'critical' || s === 'high') return 'critical';
  if (s === 'warning' || s === 'medium') return 'warning';
  return 'info';
}

function uptime(secs?: number): string {
  if (typeof secs !== 'number' || !Number.isFinite(secs)) return '—';
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  return d ? `${d}d ${h}h` : `${h}h ${Math.floor((secs % 3600) / 60)}m`;
}

export default function Overview() {
  const navigate = useNavigate();
  const [nodes, setNodes] = useState<number | null>(null);
  const [endpoints, setEndpoints] = useState<number | null>(null);
  const [endpointList, setEndpointList] = useState<{ name?: string; namespace?: string; status?: string }[]>([]);
  const [agents, setAgents] = useState<number | null>(null);
  const [agentsHealthy, setAgentsHealthy] = useState<number | null>(null);
  const [clusterStatus, setClusterStatus] = useState<string>('—');
  const [clusterScore, setClusterScore] = useState<number | null>(null);
  const [ebpf, setEbpf] = useState<Record<string, unknown> | null>(null);
  const [drops, setDrops] = useState<number | null>(null);
  const [dropList, setDropList] = useState<DropRow[]>([]);
  const [flows, setFlows] = useState<{ forwarded?: number; dropped?: number; total?: number; rps?: number; window?: number; source?: string } | null>(null);
  const [metrics, setMetrics] = useState<Record<string, unknown> | null>(null);
  const [dns, setDns] = useState<DnsRow | null>(null);
  const [anomalies, setAnomalies] = useState<AnomalyRow[] | null>(null);
  const [digest, setDigest] = useState<Digest | null>(null);
  const [err, setErr] = useState('');
  const [freshAt, setFreshAt] = useState<Partial<Record<TileKey, number>>>({});
  const [, setTick] = useState(0);

  useEffect(() => {
    const t = setInterval(() => setTick((n) => n + 1), 5_000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    let cancelled = false;
    const failed: string[] = [];
    let dropCount = 0;
    let score = 100;
    let status = 'healthy';
    let gotHealth = false;
    let gotDrops = false;

    const bumpDigest = () => {
      if (!gotHealth && !gotDrops) return;
      setDigest(buildDigest({ score, status, drops: dropCount }));
    };
    const noteErr = (msg: string) => {
      failed.push(msg);
      if (!cancelled) setErr(failed[0] ?? '');
    };
    const mark = (key: TileKey) => {
      if (!cancelled) setFreshAt((prev) => ({ ...prev, [key]: Date.now() }));
    };

    const loadFlows = () => {
      void settle(fetchFlowStats()).then((f) => {
        if (cancelled) return;
        if (f.ok) {
          const stats = f.value as {
            forwarded?: number;
            dropped?: number;
            total?: number;
            total_flows?: number;
            requests_per_second?: number;
            window_sampled?: number;
            source?: string;
            verdicts?: { forwarded?: number; dropped?: number };
          };
          setFlows({
            forwarded: stats.forwarded ?? stats.verdicts?.forwarded ?? 0,
            dropped: stats.dropped ?? stats.verdicts?.dropped ?? 0,
            total: stats.total ?? stats.total_flows,
            rps: typeof stats.requests_per_second === 'number' ? stats.requests_per_second : undefined,
            window: stats.window_sampled,
            source: stats.source,
          });
          mark('flows');
        } else noteErr(f.error);
      });
    };

    const load = () => {
      // Paint each tile as its API returns — do not wait on Promise.all.
      // Per-tile timeout keeps one slow kubectl from blanking the page.
      void settle(fetchNodes()).then((n) => {
        if (cancelled) return;
        if (n.ok) {
          setNodes((n.value as { nodes?: unknown[] }).nodes?.length ?? 0);
          mark('cluster');
        } else noteErr(n.error);
      });
      void settle(fetchEndpoints()).then((e) => {
        if (cancelled) return;
        if (e.ok) {
          const list = (e.value as { endpoints?: { name?: string; namespace?: string; status?: string }[] }).endpoints ?? [];
          setEndpoints(list.length);
          setEndpointList(list);
          mark('cluster');
        } else noteErr(e.error);
      });
      void settle(fetchCiliumStatus()).then((c) => {
        if (cancelled) return;
        if (c.ok) {
          const cv = c.value as { agents?: { ready?: boolean }[]; healthy?: number };
          const list = cv.agents ?? [];
          setAgents(list.length);
          setAgentsHealthy(cv.healthy ?? list.filter((a) => a.ready).length);
          mark('cluster');
        } else noteErr(c.error);
      });
      void settle(fetchEbpfSummary()).then((s) => {
        if (cancelled) return;
        if (s.ok) {
          setEbpf(s.value as Record<string, unknown>);
          mark('ebpf');
        } else noteErr(s.error);
      });
      void settle(fetchEbpfDrops()).then((d) => {
        if (cancelled) return;
        if (d.ok) {
          const dv = d.value as { drops?: DropRow[]; total_drops?: number };
          dropCount = dropPackets(dv);
          setDrops(dropCount);
          setDropList(dv.drops ?? []);
          gotDrops = true;
          mark('digest');
          bumpDigest();
        } else noteErr(d.error);
      });
      void settle(fetchClusterHealth()).then((h) => {
        if (cancelled) return;
        if (h.ok) {
          const hv = h.value as {
            status?: string;
            overall?: string;
            score?: number;
            health_score?: number;
          };
          status = String(hv.status ?? hv.overall ?? 'ok');
          score = hv.score ?? hv.health_score ?? 100;
          setClusterStatus(status);
          setClusterScore(score);
          gotHealth = true;
          mark('cluster');
          mark('digest');
          bumpDigest();
        } else noteErr(h.error);
      });
      void settle(fetchMetricsSummary()).then((m) => {
        if (cancelled) return;
        if (m.ok) {
          setMetrics(m.value as Record<string, unknown>);
          mark('platform');
        } else noteErr(m.error);
      });
      void settle(fetchDnsStats()).then((d) => {
        if (!cancelled && d.ok) setDns(d.value as DnsRow);
      });
      void settle(fetchAnomalies()).then((a) => {
        if (!cancelled && a.ok) setAnomalies(((a.value as { anomalies?: AnomalyRow[] }).anomalies ?? []) as AnomalyRow[]);
      });
    };

    load();
    loadFlows();
    const slow = setInterval(load, SLOW_POLL_MS);
    const fast = setInterval(loadFlows, FLOW_POLL_MS);
    return () => {
      cancelled = true;
      clearInterval(slow);
      clearInterval(fast);
    };
  }, []);

  // API get_ebpf_summary uses total_programs / total_maps; accept both names.
  const programs =
    Number(
      ebpf?.total_programs ?? ebpf?.programs_total ?? ebpf?.program_count ?? ebpf?.programs ?? 0,
    ) || 0;
  const maps =
    Number(ebpf?.total_maps ?? ebpf?.maps_total ?? ebpf?.map_count ?? ebpf?.maps ?? 0) || 0;

  const flowsTotal = flows ? (flows.total ?? (flows.forwarded ?? 0) + (flows.dropped ?? 0)) : undefined;
  const derivedRate = useRate(flowsTotal, freshAt.flows);
  const flowRate = flows?.rps ?? derivedRate;
  const windowDropShare = flows && flows.window ? (flows.dropped ?? 0) / flows.window : undefined;
  const dropRate = flowRate != null && windowDropShare != null ? flowRate * windowDropShare : undefined;
  const flowSeries = useSeries(flowRate, freshAt.flows);
  const dropSeries = useSeries(dropRate, freshAt.flows);

  const byNamespace = new Map<string, number>();
  for (const e of endpointList) byNamespace.set(e.namespace || '—', (byNamespace.get(e.namespace || '—') ?? 0) + 1);
  const rank = (m: Map<string, number>) => [...m.entries()].map(([name, count]) => ({ name, count })).sort((a, b) => b.count - a.count);
  const notReady = endpointList.filter((e) => e.status && !/ready/i.test(e.status)).length;

  const dropByReason = new Map<string, number>();
  for (const d of dropList) dropByReason.set(d.reason, (dropByReason.get(d.reason) ?? 0) + (d.count ?? 0));
  const dropReasons = rank(dropByReason);
  const policyDenied = dropByReason.get('POLICY_DENIED') ?? 0;

  const activeAnomalies = (anomalies ?? []).filter((a) => !/resolv|clos/i.test(a.status || ''));
  const criticalAnomalies = activeAnomalies.filter((a) => sevClass(a.severity) === 'critical');
  const anomalySources = new Map<string, number>();
  for (const a of activeAnomalies) {
    const who = a.source_pod ? `${a.source_namespace ? a.source_namespace + '/' : ''}${a.source_pod}` : a.source_namespace || '—';
    anomalySources.set(who, (anomalySources.get(who) ?? 0) + 1);
  }

  const dnsTotal = dns ? (dns.total_queries ?? dns.total ?? 0) : undefined;
  const dnsL7 = (dns?.l7_observed ?? 0) > 0;

  const healthTone: Tone = clusterScore == null ? 'idle' : digest && digest.severity !== 'info' ? 'warn' : scoreTone(clusterScore);
  const dropped = flows?.dropped ?? 0;
  const flowTone: Tone = !flows ? 'idle' : dropped ? 'warn' : 'ok';
  const flowTitle = !flows
    ? 'Waiting for Hubble flows.'
    : dropped
      ? `${compact(dropped)} of the last ${compact(flows.window ?? flowsTotal ?? 0)} flows dropped.`
      : `${compact(flowsTotal ?? 0)} flows, none dropped lately.`;
  const dropTone: Tone = drops == null ? 'idle' : drops ? 'warn' : 'ok';
  const anomalyTone: Tone = anomalies == null ? 'idle' : criticalAnomalies.length ? 'bad' : activeAnomalies.length ? 'warn' : 'ok';
  const dnsTone: Tone = dns == null ? 'idle' : (dns.failures ?? 0) > 0 ? 'warn' : 'ok';
  const totalRequests = Number(metrics?.total_requests ?? metrics?.request_count ?? NaN);
  const totalErrors = Number(metrics?.total_errors ?? metrics?.error_count ?? NaN);
  const cacheHits = Number(metrics?.cache_hits ?? NaN);
  const cacheMisses = Number(metrics?.cache_misses ?? NaN);
  const cacheRatio = cacheHits + cacheMisses > 0 ? Math.round((cacheHits / (cacheHits + cacheMisses)) * 100) : undefined;
  const apiTone: Tone = !metrics ? 'idle' : totalErrors > 0 ? 'warn' : 'ok';

  const topEndpoints = endpointList.slice(0, 3);
  const columns = [
    {
      title: 'Endpoints',
      nodes: (topEndpoints.length ? topEndpoints : [{ name: 'endpoints' }]).map((e) => ({
        label: e.name || 'endpoint',
        sub: e.namespace || 'pod',
        active: topEndpoints.length > 0,
        targets: [0],
      })),
    },
    {
      title: 'Cilium eBPF',
      nodes: [
        { label: 'bpf_lxc', sub: 'endpoint', active: programs > 0 },
        { label: 'bpf_host', sub: 'host', active: false },
        { label: 'bpf_overlay', sub: 'tunnel', active: false },
      ],
    },
    { title: 'Hubble', nodes: [{ label: 'hubble', sub: `${agents ?? 0} agent${agents === 1 ? '' : 's'}`, active: (flowsTotal ?? 0) > 0 || (agents ?? 0) > 0 }] },
    { title: 'Paqtra', nodes: [{ label: 'paqtra-api', sub: 'observe only', active: metrics != null || flows != null }] },
  ];

  return (
    <div className="overview">
      <header className="hero">
        <p className="eyebrow">CILIUM-NATIVE OBSERVABILITY</p>
        <h1>Trace every flow.</h1>
        <p>
          See where network traffic goes and why it is allowed or dropped — powered by Cilium eBPF. Paqtra observes; Cilium
          decides.
        </p>
        <div className="overview-hero-row">
          <StatusPill agents={agents} score={clusterScore} digest={digest} />
          <button type="button" className="primary" onClick={() => navigate('/investigate')}>
            Investigate a path
          </button>
          <button type="button" className="overview-link" onClick={() => navigate('/flows')}>
            Open Hubble flows ›
          </button>
        </div>
      </header>

      {err ? (
        <p className="warning">
          API unreachable or partial failure — showing only live responses. No demo data.
          <br />
          <span style={{ fontSize: 12, opacity: 0.85 }}>{err}</span>
        </p>
      ) : null}

      <section className="overview-stage" aria-labelledby="overview-stage-title">
        <div className="overview-stage__head">
          <h2 id="overview-stage-title">
            The datapath, live.
            <StaleBadge at={freshAt.flows ?? null} />
          </h2>
          <span className="overview-live">
            <i aria-hidden="true" /> refreshes every 5s
          </span>
        </div>
        <DatapathHero
          columns={columns}
          flowsPerSecond={flowRate}
          dropsPerSecond={dropRate}
          label="Live Cilium datapath: endpoints, Cilium eBPF programs, Hubble and the Paqtra API"
        />
        <div className="overview-pulse">
          <PulseFigure label="flows / s" value={flowRate} series={flowSeries} />
          <PulseFigure label="drops / s" value={dropRate} series={dropSeries} tone={(dropRate ?? 0) > 0 ? 'warn' : undefined} />
          <PulseFigure label="DNS flows" value={dnsTotal} />
          <PulseFigure label="health score" value={clusterScore ?? undefined} tone={clusterScore == null ? undefined : scoreTone(clusterScore)} />
        </div>
        <div className="metrics overview-totals">
          <Metric value={agents ?? '—'} label="Cilium agents" />
          <Metric value={flowsTotal ?? '—'} label="flows indexed" />
          <Metric value={drops ?? '—'} label="packets dropped" />
          <Metric value={endpoints ?? '—'} label="endpoints" />
          <Metric value={programs || '—'} label="eBPF programs" />
        </div>
      </section>

      <Chapter eyebrow="On-call digest" title={digest ? digest.headline : 'Waiting for signals…'} tone={healthTone} link="Open Health" to="/clusterhealth"
        figures={
          <>
            <Metric value={clusterScore ?? '—'} label="health score /100" />
            <Metric value={agentsHealthy != null && agents != null ? `${agentsHealthy}/${agents}` : '—'} label="agents healthy" />
            <Metric value={clusterStatus} label="cluster" />
            <Metric value={notReady} label="endpoints not ready" />
          </>
        }
      >
        <p>Cluster health, Cilium components and drop counts folded into one line for whoever is on call.</p>
        {digest && digest.whyChanged.length > 0 && (
          <ul className="overview-signals">
            {digest.whyChanged.slice(0, 3).map((w) => (
              <li key={w}>
                <span className={`severity-badge ${sevClass(digest.severity)}`}>{digest.severity}</span> {w}
              </li>
            ))}
          </ul>
        )}
      </Chapter>

      <Chapter eyebrow="Hubble flows" title={flowTitle} tone={flowTone} link="Open Flows" to="/flows" flip
        figures={
          <>
            <Metric value={flowsTotal ?? '—'} label="flows indexed" />
            <Metric value={flows?.forwarded ?? '—'} label="forwarded (recent)" />
            <Metric value={flows?.dropped ?? '—'} label="dropped (recent)" />
            <Metric value={flowRate != null ? Math.round(flowRate) : '—'} label="flows / s" />
          </>
        }
      >
        <p>Every flow from Hubble with its verdict. Cilium owns the verdict; Paqtra shows where traffic went and why.</p>
      </Chapter>

      <Chapter eyebrow="Drop diagnostics" title={drops == null ? 'Reading Cilium drop counters…' : drops ? `${compact(drops)} packets dropped by Cilium.` : 'No Cilium drops recorded.'} tone={dropTone} link="Open Drops" to="/drops"
        figures={
          <>
            <Metric value={drops ?? '—'} label="packets dropped" />
            <Metric value={dropReasons.length} label="drop reasons" />
            <Metric value={policyDenied} label="policy denied" />
            <Metric value={dropList.length} label="reason entries" />
          </>
        }
      >
        <p>Cilium's own drop reasons from its metrics map, per direction. Read-only — Cilium decided; Paqtra explains.</p>
        {dropList.length > 0 && (
          <ul className="overview-signals">
            {[...dropList].sort((a, b) => (b.count ?? 0) - (a.count ?? 0)).slice(0, 3).map((d) => (
              <li key={`${d.reason}-${d.direction ?? ''}`}>
                <span className="severity-badge warning">warning</span> {d.reason} {d.direction ? `(${d.direction})` : ''} · {(d.count ?? 0).toLocaleString()} packets
              </li>
            ))}
          </ul>
        )}
      </Chapter>

      <Chapter eyebrow="DNS" title={dns == null ? 'Waiting for DNS flows…' : `${compact(dnsTotal ?? 0)} DNS flows${dnsL7 ? '' : ', L4 only'}.`} tone={dnsTone} link="Open DNS" to="/dns" flip
        figures={
          <>
            <Metric value={dnsTotal ?? '—'} label="DNS flows" />
            <Metric value={dns?.failures ?? '—'} label="failures" />
            <Metric value={dns?.l7_observed ?? '—'} label="L7 answers" />
            <Metric value={typeof dns?.avg_latency_ms === 'number' && dnsL7 ? Number(dns.avg_latency_ms.toFixed(1)) : '—'} label="avg latency ms" />
          </>
        }
      >
        <p>
          {dns && !dnsL7
            ? 'Port-53 flows from Hubble. Enable Cilium DNS visibility to see query names, answers and latency.'
            : 'DNS queries and answers as Hubble reports them — names and rcodes, never payloads.'}
        </p>
      </Chapter>

      <Chapter eyebrow="Behavior insights" title={anomalies == null ? 'Learning normal behavior…' : activeAnomalies.length ? `${activeAnomalies.length} anomal${activeAnomalies.length === 1 ? 'y' : 'ies'} to review.` : 'Behavior matches what Paqtra learned.'} tone={anomalyTone} link="Open Anomalies" to="/anomalies"
        figures={
          <>
            <Metric value={activeAnomalies.length} label="active" />
            <Metric value={criticalAnomalies.length} label="critical" />
            <Metric value={anomalySources.size} label="sources" />
            <Metric value={new Set(activeAnomalies.map((a) => a.anomaly_type)).size} label="kinds" />
          </>
        }
      >
        <p>Baselines and drift learned from Hubble flows. Review-only — Paqtra never auto-enforces.</p>
        {activeAnomalies.length > 0 && (
          <ul className="overview-signals">
            {[...criticalAnomalies, ...activeAnomalies.filter((a) => sevClass(a.severity) !== 'critical')].slice(0, 3).map((a, i) => (
              <li key={a.id ?? i}>
                <span className={`severity-badge ${sevClass(a.severity)}`}>{a.severity || 'info'}</span>{' '}
                {(a.anomaly_type || 'anomaly').replace(/_/g, ' ')}
                {a.source_pod ? ` · ${a.source_namespace ? a.source_namespace + '/' : ''}${workloadName(a.source_pod)}` : ''}
              </li>
            ))}
          </ul>
        )}
      </Chapter>

      <Chapter eyebrow="eBPF maps" title={programs ? `${compact(programs)} programs, ${compact(maps)} maps — read-only.` : 'Read-only map inventory.'} tone={programs ? 'ok' : 'idle'} link="Open eBPF" to="/ebpf" flip
        figures={
          <>
            <Metric value={programs || '—'} label="programs" />
            <Metric value={maps || '—'} label="maps" />
            <Metric value={endpoints ?? '—'} label="identities/endpoints" />
          </>
        }
      >
        <p>Conntrack, policy map and IP cache viewers read Cilium's maps. Paqtra never writes them or attaches programs.</p>
      </Chapter>

      <Chapter eyebrow="Platform" title={metrics ? `API up ${uptime(Number(metrics.uptime_seconds))}${totalErrors > 0 ? `, ${totalErrors} errors` : ', no errors'}.` : 'Waiting for metrics summary…'} tone={apiTone} link="Open Scorecard" to="/metrics"
        figures={
          <>
            <Metric value={Number.isFinite(totalRequests) ? totalRequests : '—'} label="API requests" />
            <Metric value={Number.isFinite(totalErrors) ? totalErrors : '—'} label="errors" />
            <Metric value={cacheRatio != null ? `${cacheRatio}%` : '—'} label="cache hit" />
            <Metric value={uptime(Number(metrics?.uptime_seconds))} label="uptime" />
          </>
        }
      >
        <p>Request count, errors and cache efficiency of the Paqtra API itself.</p>
      </Chapter>

      <Reveal>
        <section className="overview-talking" aria-labelledby="overview-talking-title">
          <div className="overview-stage__head">
            <h2 id="overview-talking-title">Who is talking.</h2>
            <Link className="overview-link" to="/endpoints">
              Open Endpoints ›
            </Link>
          </div>
          <div className="overview-talking__grid">
            <RankedList title="Endpoints by namespace" items={rank(byNamespace)} empty="No Cilium endpoints yet." limit={6} />
            <RankedList title="Drop reasons" items={dropReasons} empty="No Cilium drops recorded." limit={6} />
            <RankedList title="Anomaly sources" items={rank(anomalySources).map((x) => ({ ...x, name: workloadName(x.name) }))} empty="No anomalies to review." limit={6} />
          </div>
        </section>
      </Reveal>

      <Reveal>
        <section className="overview-platform" aria-label="Datapath posture">
          <ul>
            <li><span>Datapath</span><b>Cilium eBPF</b></li>
            <li><span>Nodes</span><b>{nodes ?? '—'}</b></li>
            <li><span>Cilium agents</span><b>{agentsHealthy != null && agents != null ? `${agentsHealthy}/${agents} healthy` : '—'}</b></li>
            <li><span>Flow source</span><b>{flows?.source ? flows.source.replace(/_/g, ' ') : 'hubble'}</b></li>
            <li><span>DNS visibility</span><b>{dns == null ? '—' : dnsL7 ? 'L7' : 'L4 only'}</b></li>
            <li><span>Top drop reason</span><b>{dropReasons[0] ? `${dropReasons[0].name} · ${dropReasons[0].count.toLocaleString()}` : 'none'}</b></li>
            <li><span>Writes to Cilium</span><b>never</b></li>
          </ul>
        </section>
      </Reveal>

      <p className="overview-closing">
        Brothers with Cilium. Cilium owns CNI and policy; Paqtra is the Cilium-native observe and ops sibling; Netra remains the
        independent eBPF datapath. Paqtra never attaches its own datapath programs or writes Cilium pins.
      </p>
    </div>
  );
}

/** Strip the ReplicaSet/pod hash suffixes so a pod reads as its workload. */
function workloadName(name: string): string {
  return name.replace(/-[a-z0-9]{8,10}-[a-z0-9]{5}$/, '').replace(/-[a-z0-9]{5}$/, '');
}

function StatusPill({ agents, score, digest }: { agents: number | null; score: number | null; digest: Digest | null }) {
  if (!agents) {
    return (
      <span className="overview-status tone-idle" role="status">
        <i aria-hidden="true" /> Waiting for Cilium agents
      </span>
    );
  }
  const tone = (digest && digest.severity !== 'info') || (score != null && score < 80) ? 'warn' : 'ok';
  return (
    <span className={`overview-status tone-${tone}`} role="status">
      <i aria-hidden="true" /> Observing · {agents} Cilium agent{agents === 1 ? '' : 's'}
    </span>
  );
}

function Chapter({
  eyebrow,
  title,
  tone,
  children,
  figures,
  link,
  to,
  flip,
}: {
  eyebrow: string;
  title: string;
  tone: Tone;
  children: ReactNode;
  figures: ReactNode;
  link: string;
  to: string;
  flip?: boolean;
}) {
  return (
    <Reveal>
      <section className={`overview-chapter${flip ? ' overview-chapter--flip' : ''}`}>
        <div className="overview-chapter__copy">
          <p className="apple-eyebrow">
            <ToneDot tone={tone} />
            {eyebrow}
          </p>
          <h2>{title}</h2>
          {children}
          <Link className="overview-link" to={to}>
            {link} ›
          </Link>
        </div>
        <div className="metrics overview-chapter__figures">{figures}</div>
      </section>
    </Reveal>
  );
}
