// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import axios, { AxiosError, InternalAxiosRequestConfig } from 'axios';

/** Readable message for a failed request: prefers the API's `error` field, then `message`. */
export function apiErrorMessage(err: unknown, fallback: string): string {
  if (axios.isAxiosError(err)) {
    const data = err.response?.data as { error?: string; message?: string } | undefined;
    return data?.error ?? data?.message ?? err.message;
  }
  return fallback;
}

/** Settings stored in localStorage by the Settings page */
interface AppSettings {
  apiBaseUrl: string;
  hubbleAddress: string;
  refreshInterval: number;
  darkMode: boolean;
}

const DEFAULT_SETTINGS: AppSettings = {
  apiBaseUrl: '/api/v1',
  hubbleAddress: 'localhost:4245',
  refreshInterval: 5,
  darkMode: true,
};

function isValidApiBaseUrl(url: string): boolean {
  // Allow relative paths starting with /
  if (url.startsWith('/')) return true;
  // Only allow same-origin absolute URLs to prevent SSRF
  try {
    const parsed = new URL(url);
    return parsed.origin === window.location.origin;
  } catch {
    return false;
  }
}

function loadSettings(): AppSettings {
  try {
    const raw = localStorage.getItem('paqtra-settings');
    if (raw) {
      const merged = { ...DEFAULT_SETTINGS, ...JSON.parse(raw) };
      // Validate API base URL to prevent SSRF
      if (!isValidApiBaseUrl(merged.apiBaseUrl)) {
        console.warn('[api] Invalid apiBaseUrl in settings, using default');
        merged.apiBaseUrl = DEFAULT_SETTINGS.apiBaseUrl;
      }
      return merged;
    }
  } catch {
    // ignore
  }
  return DEFAULT_SETTINGS;
}

/**
 * Shared Axios instance with:
 * - Base URL from localStorage settings (or /api/v1)
 * - Authorization header injection
 * - Response error logging
 */
const api = axios.create({
  baseURL: loadSettings().apiBaseUrl,
  // Cluster/Hubble calls can exceed 15s under load; store-backed paths are fast,
  // but keep headroom so Overview/Flows do not flash "timeout of 15000ms exceeded".
  timeout: 45_000,
  headers: { 'Content-Type': 'application/json' },
});

// --- Request interceptor ---------------------------------------------------
api.interceptors.request.use((config: InternalAxiosRequestConfig) => {
  const token = localStorage.getItem('paqtra-token');
  if (token && config.headers) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  if (config.headers) {
    config.headers['X-CSRF-Token'] = document.querySelector('meta[name="csrf-token"]')?.getAttribute('content') || '';
  }
  return config;
});

// --- Response interceptor ---------------------------------------------------
/** Fired when the API rejects the session (expired, revoked, or the account was disabled). */
export const UNAUTHORIZED_EVENT = 'paqtra:unauthorized';

api.interceptors.response.use(
  (response) => response,
  (error: AxiosError) => {
    if (error.response?.status === 401) {
      // Token expired or invalid – clear the in-memory Authorization header
      delete api.defaults.headers.common['Authorization'];
      // A wrong password on the login form is not a lost session.
      if (!error.config?.url?.includes('/auth/login')) {
        window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
      }
      // In production, send to observability service (e.g., Sentry, Datadog)
      console.warn('[api] Unauthorized – token cleared');
    }
    return Promise.reject(error);
  },
);

export default api;

// ─── Typed API helpers ──────────────────────────────────────────────────────

// Flows
export interface FlowEndpoint {
  namespace: string;
  pod: string;
  ip: string;
  [key: string]: unknown;
}

export interface Flow {
  id: string;
  timestamp: string;
  source: FlowEndpoint;
  destination: FlowEndpoint;
  verdict: string;
  protocol: string;
  port: number;
  http_method?: string;
  http_url?: string;
  http_code?: number;
  [key: string]: unknown;
}

export interface FlowStats {
  total_flows: number;
  forwarded: number;
  dropped: number;
  redirected?: number;
  requests_per_second: number;
  avg_latency_ms: number;
  [key: string]: unknown;
}

export const fetchFlows = (params?: {
  namespace?: string;
  verdict?: string;
  limit?: number;
  offset?: number;
}) => api.get<{ flows: Flow[]; total: number }>('/flows', { params });

export const fetchFlowStats = () => api.get<FlowStats>('/flows/stats');

// Policies
export interface Policy {
  id: string;
  name: string;
  namespace: string;
  created_at: string;
  status: string;
  [key: string]: unknown;
}

export const fetchPolicies = () =>
  api.get<{ policies: Policy[] }>('/policies');

export const createPolicy = (body: {
  name: string;
  namespace: string;
  spec: unknown;
}) => api.post('/policies', body);

export const updatePolicy = (id: string, body: {
  name: string;
  namespace: string;
  spec: unknown;
}) => api.put(`/policies/${id}`, body);

export const deletePolicy = (id: string) => api.delete(`/policies/${id}`);

// Policy rules: individual ingress/egress/deny rules inside one CiliumNetworkPolicy.
export type RuleDirection = 'ingress' | 'egress' | 'ingressDeny' | 'egressDeny';
export const RULE_DIRECTIONS: RuleDirection[] = ['ingress', 'egress', 'ingressDeny', 'egressDeny'];
export type PolicyRule = Record<string, unknown>;

export interface PolicyDetail extends Policy {
  /** Optimistic-concurrency token: send it back to get a 409 if the policy changed. */
  resource_version: string;
  rule_counts: Record<RuleDirection, number>;
  spec: Record<string, unknown>;
}

export interface RuleChangeResult {
  policy: string;
  action: 'add' | 'edit' | 'delete';
  direction: RuleDirection;
  dry_run: boolean;
  rule_counts: Record<RuleDirection, number>;
  spec: Record<string, unknown>;
  result: { index: number; removed?: PolicyRule };
}

const policyPath = (id: string) => `/policies/${encodeURIComponent(id)}`;
export const fetchPolicy = (id: string) => api.get<PolicyDetail>(policyPath(id));
export const addPolicyRule = (id: string, body: { direction: RuleDirection; rule: PolicyRule; resource_version?: string }, dryRun = false) =>
  api.post<RuleChangeResult>(`${policyPath(id)}/rules`, body, { params: dryRun ? { dry_run: true } : undefined });
export const updatePolicyRule = (id: string, body: { direction: RuleDirection; index: number; rule: PolicyRule; resource_version?: string }, dryRun = false) =>
  api.put<RuleChangeResult>(`${policyPath(id)}/rules`, body, { params: dryRun ? { dry_run: true } : undefined });
export const deletePolicyRule = (id: string, q: { direction: RuleDirection; index: number; resource_version?: string }, dryRun = false) =>
  api.delete<RuleChangeResult>(`${policyPath(id)}/rules`, { params: { ...q, ...(dryRun ? { dry_run: true } : {}) } });

export const simulatePolicy = (body: {
  name: string;
  namespace: string;
  spec: unknown;
}) => api.post('/policies/simulate', body);

// Anomalies
export interface Anomaly {
  id: string;
  detected_at: string;
  severity: string;
  anomaly_type: string;
  description: string;
  source_namespace: string;
  source_pod: string | null;
  destination_namespace: string | null;
  destination_pod: string | null;
  status: string;
  remediation: string | null;
  [key: string]: unknown;
}

export const fetchAnomalies = () =>
  api.get<{ anomalies: Anomaly[]; total: number }>('/anomalies');

export const remediateAnomaly = (id: string) =>
  api.post(`/anomalies/${id}/remediate`);

// Flow history
export interface HistoryFlow {
  id: string;
  timestamp: string;
  cluster: string;
  verdict: string;
  drop_reason: string;
  protocol: string;
  port: number;
  source: { namespace: string; pod: string; ip: string };
  destination: { namespace: string; pod: string; ip: string };
}

export interface FlowCoverage {
  /** Oldest and newest stored flow the caller may see. */
  oldest: string | null;
  newest: string | null;
  stored_flows: number;
  retention_days: number;
  /** False when history is held in memory: lost on restart and capped. */
  durable: boolean;
  /** True when the range starts before the oldest stored flow; null when nothing is stored. */
  range_starts_before_oldest: boolean | null;
}

export interface FlowCapture {
  interval_secs: number;
  batch: number;
  last_capture_ok: boolean;
  last_capture_at: string | null;
  /** What the capture can miss; shown to readers verbatim. */
  note: string;
}

interface HistoryContext {
  range: { from: string; to: string };
  coverage: FlowCoverage;
  capture: FlowCapture;
}

export interface FlowHistoryResponse extends HistoryContext {
  flows: HistoryFlow[];
  total: number;
  limit: number;
  offset: number;
}

export interface TimelineBucketData {
  start: string;
  forwarded: number;
  dropped: number;
  other: number;
}

export interface FlowTimelineResponse extends HistoryContext {
  bucket_secs: number;
  total: number;
  buckets: TimelineBucketData[];
}

export interface FlowHistoryParams {
  from?: string;
  to?: string;
  namespace?: string;
  pod?: string;
  port?: number;
  verdict?: string;
  limit?: number;
  offset?: number;
}

export const fetchFlowHistory = (params: FlowHistoryParams) =>
  api.get<FlowHistoryResponse>('/flows/history', { params });

export const fetchFlowTimeline = (params: Omit<FlowHistoryParams, 'limit' | 'offset'>) =>
  api.get<FlowTimelineResponse>('/flows/history/timeline', { params });

// Compliance
export interface ComplianceFramework {
  id: string;
  name: string;
  version: string;
  description: string;
  /** Controls the framework defines. Paqtra's checks are not mapped to them. */
  control_count: number;
}

export interface AuditFinding {
  /** Paqtra's check identifier (e.g. `PCI-NET-1`), not a control id of the framework. */
  control_id: string;
  title: string;
  status: 'passed' | 'failed' | 'skipped' | string;
  severity: string;
  description: string;
}

export interface AuditSummary {
  audit_id: string;
  framework: string;
  completed_at: string | null;
  requested_by: string;
  total_controls: number;
  passed: number;
  failed: number;
  skipped: number;
  /** Share of the checks that could be evaluated which passed, in percent. */
  score: number;
}

export interface AuditResult extends AuditSummary {
  status: string;
  started_at: string;
  findings: AuditFinding[];
  cluster: string | null;
  flows_sampled: number;
}

export type ReportFormat = 'html' | 'csv' | 'json';

export const fetchFrameworks = () =>
  api.get<{ frameworks: ComplianceFramework[]; total: number }>('/compliance/frameworks');

export const runAudit = (framework: string) =>
  api.post<AuditResult>('/compliance/audit', { framework });

export const fetchAudits = () =>
  api.get<{ audits: AuditSummary[]; total: number }>('/compliance/audits');

/** Fetched as a blob because a plain link cannot carry the Authorization header. */
export const fetchAuditReport = (id: string, format: ReportFormat) =>
  api.get<Blob>(`/compliance/audits/${encodeURIComponent(id)}/report`, { params: { format }, responseType: 'blob' });

export const fetchSecurityPosture = () =>
  api.get<{ score: number; trend: string }>('/security/posture');

// Modules
export const generateAutopolicy = (body: unknown) =>
  api.post('/modules/autopolicy/generate', body);

export const fetchChaosExperiments = () =>
  api.get('/modules/chaos/experiments');

export const runChaosExperiment = (body: unknown) =>
  api.post('/modules/chaos/run', body);

export const fetchCanaryStatus = (id: string) =>
  api.get(`/modules/canary/${id}`);

// Health
export const checkHealth = () => api.get('/health', { baseURL: '/' });
export const checkReady = () => api.get('/ready');

// ─── Extended Interfaces ───────────────────────────────────────────────────────

export interface K8sEvent {
  id: string;
  type: string;
  reason: string;
  object: string;
  message: string;
  namespace: string;
  count: number;
  first_timestamp: string;
  last_timestamp: string;

  last_seen?: string;
  [key: string]: unknown;
}

export interface CiliumEndpoint {
  id: string;
  name: string;
  namespace: string;
  status: string;
  identity: string;
  ipv4: string;
  ipv6: string;
  labels: string[];
  policy_enforcement: string;
  [key: string]: unknown;
}

export interface K8sNode {
  name: string;
  status: string;
  roles: string[];
  version: string;
  os: string;
  kernel: string;
  cpu_capacity: number;
  cpu_usage: number;
  memory_capacity_gb: number;
  memory_usage_gb: number;
  pods: number;
  age: string;

  pods_capacity?: number;
  pods_count?: number;
  [key: string]: unknown;
}

export interface ReplayRecording {
  id: string;
  name: string;
  namespace: string;
  start_time: string;
  end_time: string;
  flow_count: number;
  status: string;
  size: number;

  size_bytes?: number;
  [key: string]: unknown;
}

export interface HealerProblem {
  id: string;
  type: string;
  severity: string;
  description: string;
  affected_pods: string[];
  namespace: string;
  detected_at: string;
  status: string;
  proposed_fix: string;
  /** False when Paqtra has no safe automated remediation (needs a human). */
  auto_fixable?: boolean;
  [key: string]: unknown;
}

export interface PacketDrop {
  id: string;
  timestamp: string;
  source: string;
  destination: string;
  protocol: string;
  drop_reason: string;
  root_cause: string;
  remediation: string;
  count: number;
  [key: string]: unknown;
}

export interface ClusterInfo {
  latency_ms?: number;
  name: string;
  status: string;
  endpoint: string;
  region: string;
  nodes: number;
  pods: number;
  latency: number;
  last_sync: string;
  cilium_version: string;
  [key: string]: unknown;
}

export interface HeatmapCell {
  source_namespace: string;
  dest_namespace: string;
  flow_count: number;
  dropped_count: number;
  avg_latency: number;

  destination_namespace?: string;
  avg_latency_ms?: number;
  [key: string]: unknown;
}

export interface ServiceDep {
  source: string;
  destination: string;
  protocol: string;
  port: number;
  request_count?: number;
  request_rate?: number;
  error_rate: number;
  latency_p50?: number;
  latency_p99?: number;
  [key: string]: unknown;
}

export interface SecurityFinding {
  id: string;
  severity: string;
  category: string;
  title: string;
  description: string;
  resource: string;
  namespace: string;
  status: string;
  remediation: string;
  [key: string]: unknown;
}

export interface ZeroTrustScore {
  overall: number;
  network_segmentation: number;
  identity_verification: number;
  encryption: number;
  least_privilege: number;
  monitoring: number;
  [key: string]: unknown;
}

export interface EbpfProgram {
  id: string;
  name: string;
  type: string;
  attached_to: string;
  run_count: number;
  run_time_ns: number;
  map_ids: number[];

  attach_point?: string;
  avg_run_time_ns?: number;
  map_count?: number;
  [key: string]: unknown;
}

export interface EbpfMapInfo {
  id: string;
  name: string;
  type: string;
  key_size: number;
  value_size: number;
  max_entries: number;
  current_entries: number;
  flags: string;
  [key: string]: unknown;
}

export interface MetricsSummary {
  request_count: number;
  error_count: number;
  cache_hits: number;
  cache_misses: number;
  uptime_seconds: number;

  total_errors?: number;
  total_queries?: number;
  total_requests?: number;
  [key: string]: unknown;
}

export interface HostInfo {
  hostname: string;
  os: string;
  kernel: string;
  arch: string;
  cpu_model: string;
  cpu_cores: number;
  cpu_usage: number;
  memory_total_gb: number;
  memory_used_gb: number;
  disk_total_gb: number;
  disk_used_gb: number;
  uptime_seconds: number;
  load_average: number[];
  network_interfaces: { name: string; ip: string; mac: string; speed: string; status: string }[];
}

export interface PolicyTemplate {
  id: string;
  name: string;
  category: string;
  description: string;
  yaml: string;
  tags: string[];
  [key: string]: unknown;
}

export interface DiagnosticTest {
  name: string;
  status: string;
  message: string;
  duration_ms: number;
  [key: string]: unknown;
}

export interface AuditEntry {
  id: string;
  timestamp: string;
  action: string;
  actor: string;
  resource: string;
  namespace: string;
  details: string;
  outcome: string;
  [key: string]: unknown;
}

export interface AlertRule {
  id: string;
  name: string;
  severity: string;
  condition: string;
  enabled: boolean;
  /** Not sent by the API: delivery is by severity across all enabled channels. */
  channels?: string[];
  /** Absent until the rule first fires. */
  trigger_count?: number;
  last_triggered?: string;
  [key: string]: unknown;
}

export type ChannelKind = 'webhook' | 'slack' | 'pagerduty';

export interface NotificationChannel {
  id: string;
  name: string;
  kind: ChannelKind;
  /** Masked by the API: scheme and host for URLs, first characters of a routing key. */
  target: string;
  min_severity: string | null;
  enabled: boolean;
  created_at: string;
}

export interface AlertSilence {
  id: string;
  /** Null silences every rule. */
  rule_id: string | null;
  comment: string;
  created_by: string;
  created_at: string;
  until: string;
}

export interface DeliveryResult {
  channel_id: string;
  channel_name: string;
  delivered: boolean;
  attempts: number;
  error: string | null;
}

export interface AlertEvent {
  id: string;
  rule_id: string;
  timestamp: string;
  status: string;
  severity: string;
  message: string;
  namespace: string;
  pod: string;

  rule_name?: string;
  /** True when a silence suppressed the notification for this alert. */
  silenced?: boolean;
  fired_at?: string;
  resolved_at?: string;
  [key: string]: unknown;
}

export interface ServiceNode {
  id: string;
  name: string;
  namespace: string;
  type: string;

  error_rate?: number;
  pods?: number;
  request_rate?: number;
  status?: string;
  [key: string]: unknown;
}

export interface ServiceEdge {
  source: string;
  target: string;
  protocol: string;
  port: number;
  request_rate: number;
  error_rate: number;
  flow_count?: number;
  dropped_count?: number;
  [key: string]: unknown;
}

export interface CaptureSession {
  id: string;
  name: string;
  target_pod: string;
  namespace: string;
  interface_name: string;
  filter: string;
  status: string;
  packet_count: number;
  size: number;
  started_at: string;

  size_bytes?: number;
  [key: string]: unknown;
}

export interface DnsQuery {
  id: string;
  timestamp: string;
  source_pod: string;
  namespace: string;
  query_name: string;
  query_type: string;
  response_code: string;
  response_ips: string[];
  latency_ms: number;
  [key: string]: unknown;
}

export interface DnsStats {
  total_queries: number;
  successful: number;
  nxdomain: number;
  servfail: number;
  avg_latency_ms: number;
  [key: string]: unknown;
}

export interface CiliumIdentity {
  id: number;
  labels: string[];
  namespace: string;
  endpoints_count: number;
  policy_count: number;
  created_at: string;
  [key: string]: unknown;
}

export interface MeshPeer {
  latency_ms?: number;
  name: string;
  endpoint: string;
  status: string;
  connected_since: string;
  synced_identities: number;
  synced_endpoints: number;
  synced_services: number;
  latency: number;
  [key: string]: unknown;
}

export interface BgpPeer {
  name: string;
  peer_address: string;
  peer_asn: number;
  local_asn: number;
  state: string;
  uptime: string;
  prefixes_received: number;
  prefixes_advertised: number;
  messages_received: number;
  messages_sent: number;
  [key: string]: unknown;
}

export interface BandwidthEntry {
  pod: string;
  namespace: string;
  egress_rate_mbps: number;
  ingress_rate_mbps: number;
  egress_limit_mbps: number;
  ingress_limit_mbps: number;
  total_bytes_tx: number;
  total_bytes_rx: number;
  [key: string]: unknown;
}

export interface CostBreakdown {
  namespace: string;
  cpu_cost: number;
  memory_cost: number;
  network_cost: number;
  storage_cost: number;
  total_cost: number;

  trend?: string;
  [key: string]: unknown;
}

export interface CostSummary {
  total_monthly: number;
  total_monthly_cost: number;
  total_daily: number;

  cost_trend?: string;
  savings_potential?: number;
  trend?: string;
  [key: string]: unknown;
}

export interface ForecastResult {
  metric: string;
  unit: string;
  recommendation?: string;
  points: { timestamp: string; actual?: number; predicted: number; upper_bound: number; lower_bound: number; [key: string]: unknown }[];
}

export interface EncryptionStatus {
  enabled: boolean;
  type: string;
  nodes_encrypted: number;
  nodes_total: number;
  interfaces: { name: string; peer: string; endpoint: string; latest_handshake: string; tx_bytes: number; rx_bytes: number; interface?: string; public_key?: string; node?: string; stats?: Record<string, unknown>; [key: string]: unknown }[];
  key_rotation?: string;
  key_rotation_at: string;
  [key: string]: unknown;
}

export interface LBService {
  frontend?: string;
  name: string;
  namespace: string;
  type: string;
  frontend_ip: string;
  frontend_port: number;
  protocol: string;
  backends: { ip: string; port: number; weight: number; state: string; address?: string }[];
  session_affinity: string;
  algorithm: string;
}

export interface IngressRoute {
  name: string;
  namespace: string;
  type: string;
  hosts: string[];
  paths: string[];
  tls: string;
  status: string;
  class_name: string;
  [key: string]: unknown;
}

export interface IPAMPool {
  name: string;
  cidr: string;
  total: number;
  allocated: number;
  available: number;

  usage_pct?: number;
  utilization?: number;
  [key: string]: unknown;
}

export interface IPAllocation {
  ip: string;
  pod: string;
  namespace: string;
  node: string;
  pool: string;
  [key: string]: unknown;
}

export interface LatencyBreakdown {
  service: string;
  p50: number;
  p90: number;
  p95: number;
  p99: number;
  max: number;
  histogram: { range: string; count: number; bucket?: string; [key: string]: unknown }[];
  p50_ms?: number;
  p90_ms?: number;
  p95_ms?: number;
  p99_ms?: number;
  max_ms?: number;
  [key: string]: unknown;
}

export interface MirrorRule {
  id: string;
  name: string;
  source_selector: string;
  destination: string;
  mirror_to: string;
  status: string;
  mirrored_packets: number;
  created_at: string;
  source?: string;
  mirror?: { service: string; namespace: string; port: number };

  stats?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface ClusterHealthSummary {
  status: string;
  score: number;
  node_count: number;
  pod_count: number;
  endpoint_count: number;
  components: { name: string; status: string; message: string; uptime?: string; last_check?: string; [key: string]: unknown }[];
  kubernetes?: { nodes?: number; pods?: number; endpoints?: number; version?: string; [key: string]: unknown };
  health_score?: number;
  overall?: string;
  cilium_version?: string;
  kubernetes_version?: string;
  cilium?: { version?: string; [key: string]: unknown };
  [key: string]: unknown;
}

export interface RBACBinding {
  subject: string;
  subject_kind: string;
  role: string;
  role_kind: string;
  namespace: string;
  permissions: string[];
  subjects?: { name?: string; kind?: string; namespace?: string }[];
  [key: string]: unknown;
}

export interface NetInterface {
  name: string;
  node: string;
  type: string;
  mtu: number;
  state: string;
  mac: string;
  ipv4: string;
  rx_bytes: number;
  tx_bytes: number;
  rx_packets: number;
  tx_packets: number;
  rx_errors: number;
  tx_errors: number;

  address?: string;
  addresses?: string[];
  stats?: Record<string, unknown>;
  [key: string]: unknown;
}

export interface TroubleshootResult {
  step: string;
  status: string;
  output: string;
  duration_ms: number;

  name?: string;
  [key: string]: unknown;
}

export interface WireGuardPeer {
  public_key: string;
  endpoint: string;
  allowed_ips: string[];
  latest_handshake: string;
  transfer_rx: number;
  transfer_tx: number;
  persistent_keepalive: number;
  node: string;
  [key: string]: unknown;
}

export interface CiliumAgentStatus {
  node: string;
  status: string;
  version: string;
  uptime: string;
  controllers: number;
  endpoint_count: number;
  policy_revision: number;
  proxy_redirects: number;
  identity_count: number;
  datapath: string;
  masquerading: string;
  encryption: string;
  kpr: string;

  controllers_failing?: number;
  controllers_total?: number;
  kube_proxy_replacement?: string;
  [key: string]: unknown;
}

export interface ExportConfig {
  id: string;
  name: string;
  format: string;
  destination: string;
  filter: string;
  status: string;
  exported_count: number;
  last_export: string;
  [key: string]: unknown;
}

export interface SLOTarget {
  id?: string;
  name: string;
  service: string;
  metric: string;
  target: number;
  /** Null when the latest flow sample has nothing in scope (status `no_data`). */
  current: number | null;
  /** Minutes, projected from the sample at the current error rate; null when `current` is. */
  budget_remaining: number | null;
  budget_total: number;
  window: string;
  status: string;
  sample_size?: number;
  measurement?: string;
  [key: string]: unknown;
}

export interface Incident {
  id: string;
  title: string;
  severity: string;
  status: string;
  started_at: string;
  resolved_at: string;
  duration: string;
  affected_services: string[];
  root_cause: string;
  timeline: { time: string; event: string; actor?: string; [key: string]: unknown }[];
  duration_minutes?: number;
  [key: string]: unknown;
}

export interface ChangeEntry {
  id: string;
  timestamp: string;
  type: string;
  resource: string;
  namespace: string;
  /** Not sent by the cluster-event change tracker. */
  diff_summary?: string;
  author?: string;
  rollback_available: boolean;
  rolled_back?: boolean;
  [key: string]: unknown;
}

export interface NodeDrainStatus {
  node: string;
  status: string;
  pods_evicted: number;
  pods_remaining: number;
  started_at: string;
  cordon: boolean;
  [key: string]: unknown;
}

export interface PodSecurityReport {
  namespace: string;
  enforce_level: string;
  audit_level: string;
  warn_level: string;
  total_pods: number;
  compliant_pods: number;
  violations: { pod: string; policy: string; message: string; violation?: string }[];
}

export interface EgressPolicy {
  name: string;
  namespace: string;
  gateway_node: string;
  egress_ip: string;
  destination_cidrs: string[];
  selectors: string | string[];
  status: string;
  [key: string]: unknown;
}

export interface MeshService {
  timeout_ms?: number;
  name: string;
  namespace: string;
  protocol: string;
  mtls: string;
  retries: number;
  timeout: string;
  circuit_breaker: string;
  traffic_policy: string;
  [key: string]: unknown;
}

export interface KPRStatus {
  enabled: boolean;
  mode: string;
  device: string;
  dsr_mode: string;
  session_affinity: string;
  graceful_termination: string;
  nodeport_range: string;
  services: number;
  backends: number;
  nat_entries: number;
  ct_entries: number;

  node_port_range?: string;
  [key: string]: unknown;
}

// ─── Extended API Functions ────────────────────────────────────────────────────

// Events
export const fetchEvents = () => api.get<{ events: K8sEvent[] }>('/events');

// Endpoints
export const fetchEndpoints = () => api.get<{ endpoints: CiliumEndpoint[] }>('/endpoints');

// Nodes
export const fetchNodes = () => api.get<{ nodes: K8sNode[] }>('/nodes');

// Replay / Recordings
export const fetchRecordings = () => api.get<{ recordings: ReplayRecording[] }>('/modules/replay/recordings');
export const startRecording = (body: unknown) => api.post('/modules/replay/start', body);
export const stopRecording = (id: string) => api.post(`/modules/replay/${id}/stop`);
export const fetchRecordingFlows = (id: string) => api.get(`/modules/replay/${id}/flows`);

// Healer
export const fetchHealerProblems = () => api.get<{ problems: HealerProblem[] }>('/modules/healer/problems');
export const applyHealerFix = (id: string) => api.post(`/modules/healer/${id}/fix`);

// Packet Drops / RootCause
export const fetchPacketDrops = () => api.get<{ drops: PacketDrop[] }>('/modules/rootcause/drops');
export const analyzeDrops = (_body?: unknown) => api.post('/modules/rootcause/analyze');

// Clusters
export const fetchClusters = () => api.get<{ clusters: ClusterInfo[] }>('/modules/multicluster/clusters');
export const syncClusterPolicies = (name: string) => api.post(`/modules/multicluster/${name}/sync`);

// Heatmap
export const fetchHeatmapData = () => api.get<{ cells: HeatmapCell[]; namespaces?: string[] }>('/heatmap');

// Service Dependencies
export const fetchServiceDeps = () => api.get<{ dependencies: ServiceDep[] }>('/dependencies');

// Security
export const fetchSecurityFindings = () => api.get<{ findings: SecurityFinding[] }>('/security/findings');
export const fetchZeroTrustScore = () => api.get<ZeroTrustScore>('/security/zero-trust');

// eBPF
export const fetchEbpfPrograms = () => api.get<{ programs: EbpfProgram[] }>('/modules/ebpf/programs');
export const fetchEbpfMaps = () => api.get<{ maps: EbpfMapInfo[] }>('/modules/ebpf/maps');

// Metrics
export const fetchPrometheusMetrics = () => api.get('/metrics/prometheus');
export const fetchMetricsSummary = () => api.get<MetricsSummary>('/metrics/summary');

// Host
export const fetchHostInfo = () => api.get<HostInfo>('/host/info');

// Policy Templates
export const fetchPolicyTemplates = () => api.get<{ templates: PolicyTemplate[] }>('/policies/templates');
export const applyTemplate = (id: string, _body?: unknown) => api.post(`/policies/templates/${id}/apply`);

// Diagnostics
export const runDiagnostics = () => api.post<{ tests: DiagnosticTest[] }>('/diagnostics/run');
export const fetchConnectivityTest = () => api.post('/diagnostics/connectivity');

// Audit
export const fetchAuditLog = () => api.get<{ entries: AuditEntry[] }>('/audit/log');

// Cilium / Hubble insights
export type FeatureState = 'enabled' | 'disabled' | 'set' | 'unknown';
export interface CiliumFeature {
  key: string;
  title: string;
  category: string;
  state: FeatureState;
  value?: string;
  config_key?: string;
  /** Paqtra page that shows this feature. */
  view?: string;
}
export interface HubbleNodeInfo {
  cluster: string;
  name: string;
  version: string;
  address: string;
  state: string;
  tls_enabled: boolean;
  uptime_seconds: number;
  num_flows: number;
  max_flows: number;
  seen_flows: number;
}
export interface MetricSeries { labels: Record<string, string>; value: number }
export interface MetricGroup {
  key: string;
  title: string;
  unit: string;
  series: MetricSeries[];
  hint?: string | null;
  error?: string | null;
}
export interface MetricsReport { available: boolean; reason?: string; window?: string; metrics: MetricGroup[] }
export interface CiliumResource {
  name: string | null;
  namespace: string | null;
  created_at: string | null;
  spec: unknown;
  status: unknown;
}
export const RESOURCE_KINDS: { kind: string; label: string }[] = [
  { kind: 'nodes', label: 'CiliumNodes' },
  { kind: 'egress-gateway-policies', label: 'Egress gateway policies' },
  { kind: 'bgp-peering-policies', label: 'BGP peering policies (v1)' },
  { kind: 'bgp-cluster-configs', label: 'BGP cluster configs' },
  { kind: 'bgp-peer-configs', label: 'BGP peer configs' },
  { kind: 'bgp-advertisements', label: 'BGP advertisements' },
  { kind: 'bgp-node-configs', label: 'BGP node configs' },
  { kind: 'lb-ip-pools', label: 'LB IP pools' },
  { kind: 'l2-announcement-policies', label: 'L2 announcement policies' },
  { kind: 'pod-ip-pools', label: 'Pod IP pools' },
  { kind: 'cidr-groups', label: 'CIDR groups' },
  { kind: 'gateway-classes', label: 'Gateway classes' },
  { kind: 'gateways', label: 'Gateways' },
  { kind: 'http-routes', label: 'HTTP routes' },
];
export const fetchCiliumFeatures = () =>
  api.get<{ available: boolean; reason?: string; features: CiliumFeature[] }>('/cilium/features');
export const fetchHubbleNodes = () =>
  api.get<{ available: boolean; total: number; nodes: HubbleNodeInfo[]; errors: { cluster: string; error: string }[] }>('/hubble/nodes');
export const fetchHubbleMetrics = () => api.get<MetricsReport>('/hubble/metrics');
export const fetchCiliumMetrics = () => api.get<MetricsReport>('/cilium/metrics');
export const fetchCiliumResources = (kind: string) =>
  api.get<{ kind: string; installed: boolean; total: number; items: CiliumResource[] }>(`/cilium/resources/${encodeURIComponent(kind)}`);
export const fetchAgentQuery = (what: string) =>
  api.get<{ what: string; scope: string; data: unknown }>(`/cilium/agent/${encodeURIComponent(what)}`);

// Alerts
export const fetchAlertRules = () => api.get<{ rules: AlertRule[] }>('/alerts/rules');
export const fetchAlertHistory = () => api.get<{ alerts: AlertEvent[]; events?: AlertEvent[] }>('/alerts/history');
export const toggleAlertRule = (id: string) => api.put(`/alerts/rules/${id}`);
export interface AlertRuleInput {
  name: string;
  condition: string;
  severity: string;
  enabled?: boolean;
}
export const createAlertRule = (body: AlertRuleInput) => api.post<AlertRule>('/alerts/rules', body);
export const updateAlertRule = (id: string, body: AlertRuleInput) =>
  api.put<AlertRule>(`/alerts/rules/${encodeURIComponent(id)}/definition`, body);
/** Seeded default rules are only deleted with `force`. */
export const deleteAlertRule = (id: string, force = false) =>
  api.delete(`/alerts/rules/${encodeURIComponent(id)}`, { params: force ? { force: true } : undefined });
export type UserRole = 'admin' | 'editor' | 'viewer';

export interface AppUser {
  username: string;
  role: UserRole;
  enabled: boolean;
  /** Namespaces the user is limited to; empty means all. Never set for admins. */
  namespaces: string[];
  created_at: string;
  updated_at: string;
}

export interface Me {
  username: string;
  role: string;
  /** `config` for the ADMIN_USERNAME account, `local` for stored users. */
  source: string;
  namespaces?: string[];
}

export const fetchUsers = () => api.get<{ users: AppUser[]; total: number; config_admin: string }>('/users');
export const createUser = (body: { username: string; password: string; role: UserRole; namespaces?: string[] }) => api.post('/users', body);
export const updateUser = (username: string, body: { role?: UserRole; enabled?: boolean; password?: string; namespaces?: string[] }) => api.put(`/users/${encodeURIComponent(username)}`, body);
export const deleteUser = (username: string) => api.delete(`/users/${encodeURIComponent(username)}`);
export const fetchMe = () => api.get<Me>('/auth/me');
export const changePassword = (body: { current_password: string; new_password: string }) => api.post<{ changed: boolean; reauthenticate: boolean }>('/auth/password', body);
export const fetchChannels = () => api.get<{ channels: NotificationChannel[] }>('/alerts/channels');
export const createChannel = (body: { name: string; kind: ChannelKind; target: string; min_severity?: string }) => api.post('/alerts/channels', body);
export const deleteChannel = (id: string) => api.delete(`/alerts/channels/${id}`);
export const testChannel = (id: string) => api.post<DeliveryResult>(`/alerts/channels/${id}/test`);
export const fetchSilences = () => api.get<{ silences: AlertSilence[] }>('/alerts/silences');
export const createSilence = (body: { rule_id?: string; duration_minutes: number; comment?: string }) => api.post('/alerts/silences', body);
export const deleteSilence = (id: string) => api.delete(`/alerts/silences/${id}`);

// Service Map
export const fetchServiceMap = () => api.get<{ nodes: ServiceNode[]; edges: ServiceEdge[] }>('/servicemap');

// Packet Capture
export const fetchCaptureSessions = () => api.get<{ sessions: CaptureSession[] }>('/modules/capture/sessions');
export const startCapture = (body: unknown) => api.post('/modules/capture/start', body);
export const stopCapture = (id: string) => api.post(`/modules/capture/${id}/stop`);

// DNS
export const fetchDnsQueries = (params?: { namespace?: string; query_name?: string }) =>
  api.get<{ queries: DnsQuery[] }>('/dns/queries', { params });
export const fetchDnsStats = () => api.get<DnsStats>('/dns/stats');

// Identities
export const fetchIdentities = () => api.get<{ identities: CiliumIdentity[] }>('/identities');

// Cluster Mesh
export const fetchMeshPeers = () => api.get<{ peers: MeshPeer[] }>('/clustermesh/peers');
export const connectMeshPeer = (body: unknown) => api.post('/clustermesh/connect', body);

// BGP
export const fetchBgpPeers = () => api.get<{ peers: BgpPeer[] }>('/bgp/peers');

// Bandwidth
export const fetchBandwidthData = () => api.get<{ entries: BandwidthEntry[] }>('/bandwidth');

// Cost
export const fetchCostBreakdown = () => api.get<{ breakdown: CostBreakdown[]; summary: CostSummary }>('/costs/breakdown');

// Forecast
export const fetchForecast = (body: unknown) => api.post<ForecastResult>('/forecast', body);
export const fetchForecastMetrics = () => api.get<{ metrics: string[] }>('/forecast/metrics');

// Encryption
export const fetchEncryptionStatus = () => api.get<EncryptionStatus>('/encryption/status');

// Load Balancing
export const fetchLBServices = () => api.get<{ services: LBService[] }>('/loadbalancer/services');

// Ingress
export const fetchIngressRoutes = () => api.get<{ routes: IngressRoute[] }>('/ingress/routes');

// IPAM
export const fetchIPAMPools = () => api.get<{ pools: IPAMPool[] }>('/ipam/pools');
export const fetchIPAllocations = () => api.get<{ allocations: IPAllocation[] }>('/ipam/allocations');

// Latency
export const fetchLatencyAnalysis = () => api.get<{ services: LatencyBreakdown[] }>('/latency/analysis');

// Mirror
export const fetchMirrorRules = () => api.get<{ rules: MirrorRule[] }>('/modules/mirror/rules');
export const createMirrorRule = (body: unknown) => api.post('/modules/mirror/rules', body);
export const deleteMirrorRule = (id: string) => api.delete(`/modules/mirror/rules/${id}`);

// Cluster Health
export const fetchClusterHealth = () => api.get<ClusterHealthSummary>('/cluster/health');

// RBAC
export const fetchRBACBindings = () => api.get<{ bindings: RBACBinding[] }>('/rbac/bindings');

// Network Interfaces
export const fetchNetInterfaces = () => api.get<{ interfaces: NetInterface[] }>('/network/interfaces');

// Troubleshoot
export const runTroubleshoot = (body: unknown) => api.post<{ results: TroubleshootResult[] }>('/troubleshoot/run', body);

// WireGuard
export const fetchWireGuardPeers = () => api.get<{ peers: WireGuardPeer[] }>('/wireguard/peers');

// Cilium Status
export const fetchCiliumStatus = () => api.get<{ agents: CiliumAgentStatus[] }>('/cilium/status');

// Policy Validation
export const validatePolicy = (yaml: string) => api.post('/policies/validate', { yaml });

// Export
export const fetchExportConfigs = () => api.get<{ configs: ExportConfig[] }>('/flows/exports');
export const createExportConfig = (body: unknown) => api.post('/flows/exports', body);
export const deleteExportConfig = (id: string) => api.delete(`/flows/exports/${id}`);

// SLOs
export const fetchSLOs = () => api.get<{ slos: SLOTarget[] }>('/slo/targets');
export const createSLO = (body: { name: string; target: number; window?: string; namespace?: string }) => api.post('/slo/targets', body);
export const deleteSLO = (id: string) => api.delete(`/slo/targets/${id}`);

// Incidents
export const fetchIncidents = () => api.get<{ incidents: Incident[] }>('/incidents');

// Change Log
export const fetchChangeLog = () => api.get<{ changes: ChangeEntry[]; entries?: ChangeEntry[] }>('/changes');
export const rollbackChange = (id: string) => api.post(`/changes/${id}/rollback`);
export const fetchChangeImpact = (
  id: string,
  params?: { before?: string; after?: string; kind?: string; namespace?: string; limit?: number },
) =>
  api.get(`/changes/${id}/impact`, {
    params: {
      before: params?.before ?? '30m',
      after: params?.after ?? '30m',
      kind: params?.kind || undefined,
      namespace: params?.namespace || undefined,
      limit: params?.limit,
    },
  });

export const exportInvestigateBundle = (id: string, format: 'json' | 'markdown' = 'json') =>
  api.get(`/investigate/bundles/${id}/export`, { params: { format } });

export const shareInvestigateBundle = (id: string, ttl_secs = 3600) =>
  api.post(`/investigate/bundles/${id}/share`, { ttl_secs });

export const fetchInvestigateShare = (token: string) =>
  api.get(`/investigate/share/${token}`);

export interface ConnectivityPathInput {
  name: string;
  src_namespace: string;
  src_workload: string;
  dst_namespace: string;
  dst_service: string;
  port: number;
  protocol?: string;
}

export const fetchConnectivityPaths = () => api.get('/connectivity/paths');
export const fetchConnectivityPathStatus = (id: string) =>
  api.get(`/connectivity/paths/${id}/status`);
export const createConnectivityPath = (body: ConnectivityPathInput) =>
  api.post('/connectivity/paths', body);
export const deleteConnectivityPath = (id: string) => api.delete(`/connectivity/paths/${id}`);
export const fetchConnectivityAlerts = () => api.get('/connectivity/alerts');
export const silenceConnectivityAlert = (id: string, minutes = 60) =>
  api.post(`/connectivity/alerts/${id}/silence`, { minutes });

export const fetchFlowStore = () => api.get('/flows/store');
export const purgeFlowStore = (body?: { namespace?: string; older_than_days?: number }) =>
  api.post('/flows/store/purge', body ?? {});

// Node Drain
export const fetchNodeDrainStatus = () => api.get<{ nodes: NodeDrainStatus[] }>('/nodes/drain');
export const drainNode = (node: string) => api.post('/nodes/drain', { node });
export const uncordonNode = (node: string) => api.post('/nodes/uncordon', { node });

// Pod Security
export const fetchPodSecurity = () => api.get<{ reports: PodSecurityReport[] }>('/security/pods');

// Egress Policies
export const fetchEgressPolicies = () => api.get<{ policies: EgressPolicy[] }>('/egress/policies');

// Mesh Services
export const fetchMeshServices = () => api.get<{ services: MeshService[] }>('/servicemesh/services');

// KPR (Kube Proxy Replacement)
export const fetchKPRStatus = () => api.get<KPRStatus>('/kpr/status');

// eBPF Real Data — typed interfaces

export interface ConntrackEntry { src: string; dst: string; sport: number; dport: number; proto: string; state: string; [key: string]: unknown; }
export interface IpCacheEntry { ip: string; identity: number; [key: string]: unknown; }
export interface LbBackend { address: string; port: number; [key: string]: unknown; }
export interface DropEntry { reason: string; count: number; [key: string]: unknown; }

export const fetchRealEbpfPrograms = () => api.get<{ programs: EbpfProgram[]; total: number }>('/ebpf/programs');
export const fetchRealEbpfMaps = () => api.get<{ maps: EbpfMapInfo[]; total: number }>('/ebpf/maps');
export const fetchEbpfProgramStats = (id: string) => api.get(`/ebpf/programs/${id}`);
export const fetchEbpfMapEntries = (id: number, limit?: number) => api.get(`/ebpf/maps/${id}/entries`, { params: { limit: limit || 50 } });
export const fetchEbpfConntrack = () => api.get<{ entries: ConntrackEntry[]; total: number }>('/ebpf/conntrack');
export const fetchEbpfIpcache = () => api.get<{ entries: IpCacheEntry[]; total: number }>('/ebpf/ipcache');
export const fetchEbpfLb = () => api.get<{ entries: LbBackend[]; total: number }>('/ebpf/lb');
export const fetchEbpfDrops = () => api.get<{ drops: DropEntry[]; total_drops: number }>('/ebpf/drops');
export const fetchEbpfSummary = () => api.get('/ebpf/summary');
