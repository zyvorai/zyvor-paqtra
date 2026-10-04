/** One output line of GET /api/v1/metrics/data or a stream push. */
export type ResultDim = {
  name: string;
  labels?: Record<string, string>;
  values: (number | null)[];
  anomalyRate: (number | null)[];
  series: number;
};

export type MetricResult = {
  context: string;
  title?: string;
  units?: string;
  family?: string;
  chartType?: string;
  tier: number;
  interval: number;
  after: number;
  before: number;
  timestamps: number[];
  dimensions: ResultDim[] | null;
  matched: number;
};

export type ChartInfo = { node: string; chart: string; labels?: Record<string, string>; dimensions: string[] };

export type ContextInfo = {
  context: string;
  family?: string;
  title?: string;
  units?: string;
  chartType?: string;
  charts: ChartInfo[];
  firstT: number;
  lastT: number;
};

export type StreamQuery = {
  id: string;
  context: string;
  charts?: string[];
  nodes?: string[];
  labels?: Record<string, string>;
  window: number;
  points: number;
  groupBy?: string;
  group?: string;
  aggregate?: string;
};

/** The order families appear in the sidebar; anything else follows, sorted. */
const familyOrder = ['cpu', 'load', 'ram', 'swap', 'pressure', 'disk', 'net', 'ip', 'tcp', 'udp', 'ipv4', 'ipv6', 'softnet', 'connection tracker', 'flows', 'policy', 'drops', 'http', 'dns', 'processes', 'apps'];

export function familyOf(c: ContextInfo): string {
  if (c.family) return c.family;
  const dot = c.context.indexOf('.');
  return dot > 0 ? c.context.slice(0, dot) : c.context;
}

/** Top-level section: system, network, workloads, ebpf, apps, other. */
export function sectionOf(context: string): string {
  const head = context.split('.')[0];
  if (head === 'system' || head === 'cpu' || head === 'mem' || head === 'disk') return 'System';
  if (head === 'net' || head === 'ip' || head === 'ipv4' || head === 'ipv6' || head === 'netfilter') return 'Network';
  if (head === 'cgroup' || head === 'app' || head === 'apps') return 'Workloads';
  if (head === 'ebpf') return 'eBPF datapath';
  if (head === 'hubble') return 'Hubble';
  return 'Applications';
}

export function groupContexts(cs: ContextInfo[]): { section: string; families: { family: string; contexts: ContextInfo[] }[] }[] {
  const sections = new Map<string, Map<string, ContextInfo[]>>();
  for (const c of cs) {
    const s = sectionOf(c.context);
    const f = familyOf(c);
    if (!sections.has(s)) sections.set(s, new Map());
    const fam = sections.get(s)!;
    if (!fam.has(f)) fam.set(f, []);
    fam.get(f)!.push(c);
  }
  const sectionRank = ['System', 'Network', 'Workloads', 'Hubble', 'eBPF datapath', 'Applications'];
  const famRank = (f: string) => {
    const i = familyOrder.indexOf(f);
    return i < 0 ? familyOrder.length : i;
  };
  return [...sections.entries()]
    .sort((a, b) => sectionRank.indexOf(a[0]) - sectionRank.indexOf(b[0]))
    .map(([section, fams]) => ({
      section,
      families: [...fams.entries()]
        .sort((a, b) => famRank(a[0]) - famRank(b[0]) || a[0].localeCompare(b[0]))
        .map(([family, contexts]) => ({ family, contexts: contexts.sort((x, y) => x.context.localeCompare(y.context)) })),
    }));
}

/** y-axis range over every non-null value; stacked charts sum per point. */
export function valueRange(r: MetricResult, stacked: boolean): [number, number] {
  const dims = r.dimensions || [];
  let lo = Infinity;
  let hi = -Infinity;
  if (stacked) {
    for (let i = 0; i < r.timestamps.length; i++) {
      let pos = 0;
      let neg = 0;
      for (const d of dims) {
        const v = d.values[i];
        if (v == null) continue;
        if (v >= 0) pos += v;
        else neg += v;
      }
      hi = Math.max(hi, pos);
      lo = Math.min(lo, neg);
    }
  } else {
    for (const d of dims)
      for (const v of d.values) {
        if (v == null) continue;
        lo = Math.min(lo, v);
        hi = Math.max(hi, v);
      }
  }
  if (!isFinite(lo) || !isFinite(hi)) return [0, 1];
  if (r.units === '%' && hi <= 100 && lo >= 0) return [0, Math.max(hi, 1) > 50 ? 100 : niceCeil(hi)];
  lo = Math.min(lo, 0);
  if (hi === lo) hi = lo + 1;
  return [lo, niceCeil(hi)];
}

export function niceCeil(v: number): number {
  if (v <= 0) return v === 0 ? 1 : -niceFloor(-v);
  const p = Math.pow(10, Math.floor(Math.log10(v)));
  for (const m of [1, 1.2, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10]) if (m * p >= v) return m * p;
  return 10 * p;
}

function niceFloor(v: number): number {
  const p = Math.pow(10, Math.floor(Math.log10(v)));
  return Math.floor(v / p) * p;
}

const si = ['', 'k', 'M', 'G', 'T', 'P'];

export function formatValue(v: number | null | undefined, units = ''): string {
  if (v == null || !isFinite(v)) return '—';
  const a = Math.abs(v);
  if (units === '%') return `${v.toFixed(a < 10 ? 1 : 0)}%`;
  let i = 0;
  let x = v;
  while (Math.abs(x) >= 1000 && i < si.length - 1) {
    x /= 1000;
    i++;
  }
  const digits = Math.abs(x) >= 100 || Number.isInteger(x) ? 0 : Math.abs(x) >= 10 ? 1 : 2;
  return `${x.toFixed(digits)}${si[i]}${units ? ' ' + units : ''}`;
}

export function formatTime(t: number, span: number): string {
  const d = new Date(t * 1000);
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  const ss = String(d.getSeconds()).padStart(2, '0');
  if (span <= 3600) return `${hh}:${mm}:${ss}`;
  if (span <= 86400) return `${hh}:${mm}`;
  return `${d.getMonth() + 1}/${d.getDate()} ${hh}:${mm}`;
}

/** Latest non-null value of a dimension. */
export function latest(d: ResultDim): number | null {
  for (let i = d.values.length - 1; i >= 0; i--) if (d.values[i] != null) return d.values[i];
  return null;
}

/** Highest anomaly rate across dimensions at each point, 0..100 or null. */
export function anomalyBand(r: MetricResult): (number | null)[] {
  const dims = r.dimensions || [];
  return r.timestamps.map((_, i) => {
    let m: number | null = null;
    for (const d of dims) {
      const a = d.anomalyRate[i];
      if (a != null) m = Math.max(m ?? 0, a);
    }
    return m;
  });
}

export function streamId(context: string, node: string, chart?: string): string {
  return [context, node || '*', chart || '*'].join('|');
}

/** ws(s)://host/api/v1/ws/metrics/live for the current page origin. */
export function streamSocketURL(loc: { protocol: string; host: string }, token?: string | null): string {
  const q = token ? `?token=${encodeURIComponent(token)}` : '';
  return `${loc.protocol === 'https:' ? 'wss' : 'ws'}://${loc.host}/api/v1/ws/metrics/live${q}`;
}

export const windows: { label: string; seconds: number }[] = [
  { label: '5m', seconds: 300 },
  { label: '15m', seconds: 900 },
  { label: '1h', seconds: 3600 },
  { label: '6h', seconds: 21600 },
  { label: '24h', seconds: 86400 },
  { label: '7d', seconds: 604800 },
  { label: '30d', seconds: 2592000 },
];

/** Live 1-second streaming is used up to one hour; longer windows poll. */
export const streamMaxWindow = 3600;

export const palette = ['#3b82f6', '#22c55e', '#f59e0b', '#ef4444', '#a855f7', '#06b6d4', '#ec4899', '#84cc16', '#f97316', '#14b8a6', '#6366f1', '#eab308'];

/** Charts drawn per instance before a context collapses into one chart. */
const MAX_INSTANCES = 6;
const MAX_CHARTS = 40;

export type ChartSpec = { q: StreamQuery; title: string; subtitle: string };

export function chartsFor(contexts: ContextInfo[], node: string, split: string, window: number): ChartSpec[] {
  const points = window <= 900 ? Math.min(window, 300) : 360;
  const out: ChartSpec[] = [];
  for (const c of contexts) {
    const charts = c.charts.filter((ch) => !node || ch.node === node);
    const ids = [...new Set(charts.map((ch) => ch.chart))];
    const nodes = node ? [node] : undefined;
    if (node && ids.length > 1 && ids.length <= MAX_INSTANCES) {
      for (const id of ids) {
        const inst = charts.find((ch) => ch.chart === id);
        const lbl = inst?.labels ? Object.entries(inst.labels).map(([k, v]) => `${k}=${v}`).join(' ') : '';
        out.push({ q: { id: streamId(c.context, node, id), context: c.context, charts: [id], nodes, window, points }, title: c.title || c.context, subtitle: lbl || id });
      }
      continue;
    }
    const groupBy = ids.length > MAX_INSTANCES ? 'chart' : split;
    out.push({
      q: { id: streamId(c.context, node, groupBy), context: c.context, nodes, window, points, groupBy },
      title: c.title || c.context,
      subtitle: `${c.context}${ids.length > 1 ? ` · ${ids.length} instances` : ''}${groupBy !== 'dimension' ? ` · by ${groupBy}` : ''}`,
    });
  }
  return out.slice(0, MAX_CHARTS);
}

export type Ranked = { node: string; context: string; chart: string; dimension: string; units?: string; anomalyRate: number; score: number };
export type NodeRate = { node: string; anomalyRate: number; dimensions: number; anomalousDimensions: number; timeline?: { t: number; rate: number | null }[] };
export type AnomalySummary = { after: number; before: number; nodes: NodeRate[] | null; ranked: Ranked[] | null };

/** The per-node anomaly timeline as a chart result, one line per node. */
export function timelineResult(s: AnomalySummary): MetricResult | undefined {
  const nodes = s.nodes || [];
  const ts = nodes[0]?.timeline?.map((p) => p.t) || [];
  if (!ts.length) return undefined;
  return {
    context: 'anomaly.rate',
    title: 'Node anomaly rate',
    units: '%',
    tier: 0,
    interval: ts.length > 1 ? ts[1] - ts[0] : 1,
    after: s.after,
    before: s.before,
    timestamps: ts,
    dimensions: nodes.map((n) => ({ name: n.node, values: (n.timeline || []).map((p) => p.rate), anomalyRate: (n.timeline || []).map(() => null), series: n.dimensions })),
    matched: nodes.length,
  };
}

/** Age of a Unix-seconds timestamp, compact. */
export function since(t: number, now = Date.now()): string {
  const s = Math.max(0, Math.round(now / 1000 - t));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.round(s / 60)}m`;
  if (s < 86400) return `${(s / 3600).toFixed(1)}h`;
  return `${(s / 86400).toFixed(1)}d`;
}
