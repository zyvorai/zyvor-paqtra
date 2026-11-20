// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { test, expect } from '@playwright/test';

const KEY_VIEWS = [
  { path: '/', heading: 'Dashboard' },
  { path: '/flows', heading: 'Flow Monitoring' },
  { path: '/topology', heading: 'Network Topology' },
  { path: '/policies', heading: 'Policy Management' },
  { path: '/anomalies', heading: 'Anomaly Detection' },
  { path: '/compliance', heading: 'Security & Compliance' },
  { path: '/events', heading: 'Events' },
  { path: '/nodes', heading: 'Nodes' },
  { path: '/endpoints', heading: 'Cilium Endpoints' },
  { path: '/autopolicy', heading: 'AutoPolicy Engine' },
  { path: '/chaos', heading: 'Chaos Engineering' },
  { path: '/canary', heading: 'Canary Deployments' },
  { path: '/security', heading: 'Security Dashboard' },
  { path: '/heatmap', heading: 'Traffic Heatmap' },
  { path: '/ebpf', heading: 'eBPF Profiler' },
  { path: '/diagnostics', heading: 'Network Diagnostics' },
  { path: '/settings', heading: 'Settings' },
  { path: '/servicemap', heading: 'Service Map' },
  { path: '/policy-editor', heading: 'Policy Editor' },
  { path: '/slo', heading: 'SLO Dashboard' },
  { path: '/clusterhealth', heading: 'Cluster Health' },
];

for (const view of KEY_VIEWS) {
  test(`${view.path} loads with correct heading`, async ({ page }) => {
    await page.goto(view.path);
    await expect(page.locator('h1')).toContainText(view.heading, { timeout: 10_000 });
  });
}
