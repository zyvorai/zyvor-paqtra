import React, { Suspense, useEffect, useState } from 'react';
import { BrowserRouter as Router, Routes, Route } from 'react-router-dom';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import MainLayout from './components/MainLayout';
import ErrorBoundary from './components/ErrorBoundary';
import LoadingSpinner from './components/LoadingSpinner';
import LoginPage from './components/LoginPage';
import { ToastProvider } from './components/Toast';
import { useAuthStore } from './stores/authStore';
import { applyTheme, readStoredTheme } from './theme';

// Lazy-loaded views
const Dashboard = React.lazy(() => import('./views/Dashboard'));
const Flows = React.lazy(() => import('./views/Flows'));
const InvestigatePath = React.lazy(() => import('./views/InvestigatePath'));
const Topology = React.lazy(() => import('./views/Topology'));
const Policies = React.lazy(() => import('./views/Policies'));
const Anomalies = React.lazy(() => import('./views/Anomalies'));
const Compliance = React.lazy(() => import('./views/Compliance'));
const AutoPolicy = React.lazy(() => import('./views/AutoPolicy'));
const Chaos = React.lazy(() => import('./views/Chaos'));
const Canary = React.lazy(() => import('./views/Canary'));
const Events = React.lazy(() => import('./views/Events'));
const Endpoints = React.lazy(() => import('./views/Endpoints'));
const Nodes = React.lazy(() => import('./views/Nodes'));
const Replay = React.lazy(() => import('./views/Replay'));
const Healer = React.lazy(() => import('./views/Healer'));
const RootCause = React.lazy(() => import('./views/RootCause'));
const MultiCluster = React.lazy(() => import('./views/MultiCluster'));
const Heatmap = React.lazy(() => import('./views/Heatmap'));
const ServiceDeps = React.lazy(() => import('./views/ServiceDeps'));
const SecurityDash = React.lazy(() => import('./views/SecurityDash'));
const EbpfProfiler = React.lazy(() => import('./views/EbpfProfiler'));
const MetricsDash = React.lazy(() => import('./views/MetricsDash'));
const NodeMetrics = React.lazy(() => import('./views/NodeMetrics'));
const MetricAnomalies = React.lazy(() => import('./views/MetricAnomalies'));
const MetricAlerts = React.lazy(() => import('./views/MetricAlerts'));
const HostInfo = React.lazy(() => import('./views/HostInfo'));
const PolicyTemplates = React.lazy(() => import('./views/PolicyTemplates'));
const Diagnostics = React.lazy(() => import('./views/Diagnostics'));
const AuditLog = React.lazy(() => import('./views/AuditLog'));
const Alerts = React.lazy(() => import('./views/Alerts'));
const ServiceMap = React.lazy(() => import('./views/ServiceMap'));
const PacketCapture = React.lazy(() => import('./views/PacketCapture'));
const DnsMonitor = React.lazy(() => import('./views/DnsMonitor'));
const Identities = React.lazy(() => import('./views/Identities'));
const ClusterMesh = React.lazy(() => import('./views/ClusterMesh'));
const BgpPeering = React.lazy(() => import('./views/BgpPeering'));
const Bandwidth = React.lazy(() => import('./views/Bandwidth'));
const CostAnalytics = React.lazy(() => import('./views/CostAnalytics'));
const Forecasting = React.lazy(() => import('./views/Forecasting'));
const EncryptionView = React.lazy(() => import('./views/Encryption'));
const LoadBalancerView = React.lazy(() => import('./views/LoadBalancer'));
const IngressGateway = React.lazy(() => import('./views/IngressGateway'));
const IPAMView = React.lazy(() => import('./views/IPAM'));
const LatencyAnalysis = React.lazy(() => import('./views/LatencyAnalysis'));
const TrafficMirror = React.lazy(() => import('./views/TrafficMirror'));
const ClusterHealthView = React.lazy(() => import('./views/ClusterHealth'));
const RBACVisualizer = React.lazy(() => import('./views/RBACVisualizer'));
const NetworkIfaces = React.lazy(() => import('./views/NetworkIfaces'));
const TroubleshootView = React.lazy(() => import('./views/Troubleshoot'));
const WireGuardPeers = React.lazy(() => import('./views/WireGuardPeers'));
const CiliumStatusView = React.lazy(() => import('./views/CiliumStatus'));
const PolicyEditorView = React.lazy(() => import('./views/PolicyEditor'));
const RuleBuilderView = React.lazy(() => import('./views/RuleBuilder'));
const PolicyRulesView = React.lazy(() => import('./views/PolicyRules'));
const CiliumInsightsView = React.lazy(() => import('./views/CiliumInsights'));
const FlowExporter = React.lazy(() => import('./views/FlowExporter'));
const SLODashboard = React.lazy(() => import('./views/SLODashboard'));
const IncidentTimeline = React.lazy(() => import('./views/IncidentTimeline'));
const ChangeLogView = React.lazy(() => import('./views/ChangeLog'));
const ConnectivityChecks = React.lazy(() => import('./views/ConnectivityChecks'));
const NodeDrainView = React.lazy(() => import('./views/NodeDrain'));
const PodSecurityView = React.lazy(() => import('./views/PodSecurity'));
const EgressGatewayView = React.lazy(() => import('./views/EgressGateway'));
const ServiceMeshViewComp = React.lazy(() => import('./views/ServiceMeshView'));
const KubeProxyReplacement = React.lazy(() => import('./views/KubeProxyReplacement'));
const ConntrackViewer = React.lazy(() => import('./views/ConntrackViewer'));
const PolicyMapViewer = React.lazy(() => import('./views/PolicyMapViewer'));
const IPCacheViewer = React.lazy(() => import('./views/IPCacheViewer'));
const LBMapViewer = React.lazy(() => import('./views/LBMapViewer'));
const DropDashboard = React.lazy(() => import('./views/DropDashboard'));
const Settings = React.lazy(() => import('./views/Settings'));
const Users = React.lazy(() => import('./views/Users'));
const FlowHistory = React.lazy(() => import('./views/FlowHistory'));
const NotFound = React.lazy(() => import('./views/NotFound'));

/** Wrap each route element in its own ErrorBoundary so a single view crash
 *  doesn't take down the entire app. */
function ViewBoundary({ children }: { children: React.ReactNode }) {
  return <ErrorBoundary>{children}</ErrorBoundary>;
}

/** Helper: wrap a lazy-loaded view in Suspense + per-view ErrorBoundary */
function V({ children }: { children: React.ReactNode }) {
  return <ViewBoundary>{children}</ViewBoundary>;
}

const App: React.FC = () => {
  const [queryClient] = useState(() => new QueryClient({
    defaultOptions: {
      queries: { retry: 1, refetchOnWindowFocus: false },
    },
  }));
  const { authRequired, sessionReady, checkSession } = useAuthStore();

  useEffect(() => {
    applyTheme(readStoredTheme());
  }, []);

  useEffect(() => {
    checkSession();
  }, [checkSession]);

  if (!sessionReady) {
    return <LoadingSpinner size="lg" text="Loading..." fullScreen />;
  }

  if (authRequired) {
    return <LoginPage />;
  }

  return (
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <ErrorBoundary>
          <Router>
            <Suspense fallback={<LoadingSpinner size="lg" text="Loading..." fullScreen />}>
              <Routes>
                <Route element={<MainLayout />}>
                  <Route path="/" element={<V><Dashboard /></V>} />
                  <Route path="/investigate" element={<V><InvestigatePath /></V>} />
                  <Route path="/flows" element={<V><Flows /></V>} />
                  <Route path="/topology" element={<V><Topology /></V>} />
                  <Route path="/policies" element={<V><Policies /></V>} />
                  <Route path="/anomalies" element={<V><Anomalies /></V>} />
                  <Route path="/compliance" element={<V><Compliance /></V>} />
                  <Route path="/autopolicy" element={<V><AutoPolicy /></V>} />
                  <Route path="/chaos" element={<V><Chaos /></V>} />
                  <Route path="/canary" element={<V><Canary /></V>} />
                  <Route path="/events" element={<V><Events /></V>} />
                  <Route path="/endpoints" element={<V><Endpoints /></V>} />
                  <Route path="/nodes" element={<V><Nodes /></V>} />
                  <Route path="/replay" element={<V><Replay /></V>} />
                  <Route path="/healer" element={<V><Healer /></V>} />
                  <Route path="/rootcause" element={<V><RootCause /></V>} />
                  <Route path="/multicluster" element={<V><MultiCluster /></V>} />
                  <Route path="/heatmap" element={<V><Heatmap /></V>} />
                  <Route path="/dependencies" element={<V><ServiceDeps /></V>} />
                  <Route path="/security" element={<V><SecurityDash /></V>} />
                  <Route path="/ebpf" element={<V><EbpfProfiler /></V>} />
                  <Route path="/metrics" element={<V><MetricsDash /></V>} />
                  <Route path="/node-metrics" element={<V><NodeMetrics /></V>} />
                  <Route path="/metric-anomalies" element={<V><MetricAnomalies /></V>} />
                  <Route path="/metric-alerts" element={<V><MetricAlerts /></V>} />
                  <Route path="/host" element={<V><HostInfo /></V>} />
                  <Route path="/templates" element={<V><PolicyTemplates /></V>} />
                  <Route path="/diagnostics" element={<V><Diagnostics /></V>} />
                  <Route path="/audit" element={<V><AuditLog /></V>} />
                  <Route path="/alerts" element={<V><Alerts /></V>} />
                  <Route path="/servicemap" element={<V><ServiceMap /></V>} />
                  <Route path="/capture" element={<V><PacketCapture /></V>} />
                  <Route path="/dns" element={<V><DnsMonitor /></V>} />
                  <Route path="/identities" element={<V><Identities /></V>} />
                  <Route path="/clustermesh" element={<V><ClusterMesh /></V>} />
                  <Route path="/bgp" element={<V><BgpPeering /></V>} />
                  <Route path="/bandwidth" element={<V><Bandwidth /></V>} />
                  <Route path="/costs" element={<V><CostAnalytics /></V>} />
                  <Route path="/forecast" element={<V><Forecasting /></V>} />
                  <Route path="/encryption" element={<V><EncryptionView /></V>} />
                  <Route path="/loadbalancer" element={<V><LoadBalancerView /></V>} />
                  <Route path="/ingress" element={<V><IngressGateway /></V>} />
                  <Route path="/ipam" element={<V><IPAMView /></V>} />
                  <Route path="/latency" element={<V><LatencyAnalysis /></V>} />
                  <Route path="/mirror" element={<V><TrafficMirror /></V>} />
                  <Route path="/clusterhealth" element={<V><ClusterHealthView /></V>} />
                  <Route path="/rbac" element={<V><RBACVisualizer /></V>} />
                  <Route path="/interfaces" element={<V><NetworkIfaces /></V>} />
                  <Route path="/troubleshoot" element={<V><TroubleshootView /></V>} />
                  <Route path="/wireguard" element={<V><WireGuardPeers /></V>} />
                  <Route path="/cilium-status" element={<V><CiliumStatusView /></V>} />
                  <Route path="/policy-editor" element={<V><PolicyEditorView /></V>} />
                  <Route path="/rule-builder" element={<V><RuleBuilderView /></V>} />
                  <Route path="/policy-rules" element={<V><PolicyRulesView /></V>} />
                  <Route path="/cilium-insights" element={<V><CiliumInsightsView /></V>} />
                  <Route path="/flow-export" element={<V><FlowExporter /></V>} />
                  <Route path="/slo" element={<V><SLODashboard /></V>} />
                  <Route path="/incidents" element={<V><IncidentTimeline /></V>} />
                  <Route path="/changelog" element={<V><ChangeLogView /></V>} />
                  <Route path="/connectivity" element={<V><ConnectivityChecks /></V>} />
                  <Route path="/node-drain" element={<V><NodeDrainView /></V>} />
                  <Route path="/pod-security" element={<V><PodSecurityView /></V>} />
                  <Route path="/egress" element={<V><EgressGatewayView /></V>} />
                  <Route path="/service-mesh" element={<V><ServiceMeshViewComp /></V>} />
                  <Route path="/kpr" element={<V><KubeProxyReplacement /></V>} />
                  <Route path="/conntrack" element={<V><ConntrackViewer /></V>} />
                  <Route path="/policy-map" element={<V><PolicyMapViewer /></V>} />
                  <Route path="/ipcache" element={<V><IPCacheViewer /></V>} />
                  <Route path="/lb-map" element={<V><LBMapViewer /></V>} />
                  <Route path="/drops" element={<V><DropDashboard /></V>} />
                  <Route path="/settings" element={<V><Settings /></V>} />
                  <Route path="/users" element={<V><Users /></V>} />
                  <Route path="/flows/history" element={<V><FlowHistory /></V>} />
                  <Route path="*" element={<V><NotFound /></V>} />
                </Route>
              </Routes>
            </Suspense>
          </Router>
        </ErrorBoundary>
      </ToastProvider>
    </QueryClientProvider>
  );
};

export default App;
