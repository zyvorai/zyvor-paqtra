// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useEffect, useRef, useCallback } from 'react';
import { Search, X, ArrowRight, Network, Shield, Bug, Zap } from 'lucide-react';
import { useNavigate } from 'react-router-dom';

interface SearchResult {
  label: string;
  path: string;
  category: string;
  icon: React.ReactNode;
}

const ALL_RESULTS: SearchResult[] = [
  // Overview
  { label: 'Dashboard', path: '/', category: 'Overview', icon: <Zap className="w-4 h-4" /> },
  { label: 'Events', path: '/events', category: 'Overview', icon: <Zap className="w-4 h-4" /> },
  { label: 'Nodes', path: '/nodes', category: 'Overview', icon: <Zap className="w-4 h-4" /> },
  { label: 'Cilium Endpoints', path: '/endpoints', category: 'Overview', icon: <Zap className="w-4 h-4" /> },
  // Observability
  { label: 'Flow Monitoring', path: '/flows', category: 'Observability', icon: <Network className="w-4 h-4" /> },
  { label: 'Network Topology', path: '/topology', category: 'Observability', icon: <Network className="w-4 h-4" /> },
  { label: 'Traffic Heatmap', path: '/heatmap', category: 'Observability', icon: <Network className="w-4 h-4" /> },
  { label: 'Service Dependencies', path: '/dependencies', category: 'Observability', icon: <Network className="w-4 h-4" /> },
  { label: 'API Metrics', path: '/metrics', category: 'Observability', icon: <Zap className="w-4 h-4" /> },
  // Security
  { label: 'Policy Management', path: '/policies', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  { label: 'Anomaly Detection', path: '/anomalies', category: 'Security', icon: <Bug className="w-4 h-4" /> },
  { label: 'Security Dashboard', path: '/security', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  { label: 'Compliance', path: '/compliance', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  // Intelligence
  { label: 'AutoPolicy Engine', path: '/autopolicy', category: 'Intelligence', icon: <Zap className="w-4 h-4" /> },
  { label: 'Network Healer', path: '/healer', category: 'Intelligence', icon: <Zap className="w-4 h-4" /> },
  { label: 'Root Cause Analysis', path: '/rootcause', category: 'Intelligence', icon: <Zap className="w-4 h-4" /> },
  { label: 'Host Information', path: '/host', category: 'Overview', icon: <Zap className="w-4 h-4" /> },
  // Observability (continued)
  { label: 'Service Map', path: '/servicemap', category: 'Observability', icon: <Network className="w-4 h-4" /> },
  { label: 'DNS Monitor', path: '/dns', category: 'Observability', icon: <Network className="w-4 h-4" /> },
  { label: 'Bandwidth Manager', path: '/bandwidth', category: 'Observability', icon: <Network className="w-4 h-4" /> },
  // Security (continued)
  { label: 'Policy Templates', path: '/templates', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  { label: 'Security Identities', path: '/identities', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  { label: 'Audit Log', path: '/audit', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  { label: 'Alerts', path: '/alerts', category: 'Security', icon: <Bug className="w-4 h-4" /> },
  // Intelligence (continued)
  { label: 'Network Diagnostics', path: '/diagnostics', category: 'Intelligence', icon: <Zap className="w-4 h-4" /> },
  // Operations
  { label: 'Chaos Engineering', path: '/chaos', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'Canary Deployments', path: '/canary', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'Flow Replay', path: '/replay', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'Packet Capture', path: '/capture', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'Multi-Cluster', path: '/multicluster', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'Cluster Mesh', path: '/clustermesh', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'BGP Peering', path: '/bgp', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'Traffic Mirroring', path: '/mirror', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'eBPF Profiler', path: '/ebpf', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  // Networking
  { label: 'Load Balancer', path: '/loadbalancer', category: 'Networking', icon: <Network className="w-4 h-4" /> },
  { label: 'Ingress & Gateway', path: '/ingress', category: 'Networking', icon: <Network className="w-4 h-4" /> },
  { label: 'IP Address Management', path: '/ipam', category: 'Networking', icon: <Network className="w-4 h-4" /> },
  { label: 'Network Interfaces', path: '/interfaces', category: 'Networking', icon: <Network className="w-4 h-4" /> },
  // Observability (more)
  { label: 'Latency Analysis', path: '/latency', category: 'Observability', icon: <Network className="w-4 h-4" /> },
  // Security (more)
  { label: 'Encryption Status', path: '/encryption', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  { label: 'RBAC Visualizer', path: '/rbac', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  // Intelligence (more)
  { label: 'Troubleshoot', path: '/troubleshoot', category: 'Intelligence', icon: <Zap className="w-4 h-4" /> },
  { label: 'Capacity Forecasting', path: '/forecast', category: 'Intelligence', icon: <Zap className="w-4 h-4" /> },
  { label: 'Cluster Health', path: '/clusterhealth', category: 'Overview', icon: <Zap className="w-4 h-4" /> },
  // FinOps
  { label: 'Cost Analytics', path: '/costs', category: 'FinOps', icon: <Zap className="w-4 h-4" /> },
  // Batch 5
  { label: 'WireGuard Peers', path: '/wireguard', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  { label: 'Cilium Agent Status', path: '/cilium-status', category: 'Overview', icon: <Zap className="w-4 h-4" /> },
  { label: 'Policy Editor', path: '/policy-editor', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  { label: 'Policy Rules', path: '/policy-rules', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  { label: 'Cilium Insights', path: '/cilium-insights', category: 'Overview', icon: <Shield className="w-4 h-4" /> },
  { label: 'Flow Exporter', path: '/flow-export', category: 'Observability', icon: <Network className="w-4 h-4" /> },
  { label: 'SLO Dashboard', path: '/slo', category: 'Observability', icon: <Zap className="w-4 h-4" /> },
  { label: 'Incident Timeline', path: '/incidents', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'Change Log', path: '/changelog', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'Node Drain', path: '/node-drain', category: 'Operations', icon: <Zap className="w-4 h-4" /> },
  { label: 'Pod Security Standards', path: '/pod-security', category: 'Security', icon: <Shield className="w-4 h-4" /> },
  { label: 'Egress Gateway', path: '/egress', category: 'Networking', icon: <Network className="w-4 h-4" /> },
  { label: 'Service Mesh', path: '/service-mesh', category: 'Networking', icon: <Network className="w-4 h-4" /> },
  { label: 'KubeProxy Replacement', path: '/kpr', category: 'Networking', icon: <Network className="w-4 h-4" /> },
  // System
  { label: 'Settings', path: '/settings', category: 'System', icon: <Zap className="w-4 h-4" /> },
];

interface Props {
  isOpen: boolean;
  onClose: () => void;
}

const GlobalSearch: React.FC<Props> = ({ isOpen, onClose }) => {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const navigate = useNavigate();

  const filtered = query.trim()
    ? ALL_RESULTS.filter(
        (r) =>
          r.label.toLowerCase().includes(query.toLowerCase()) ||
          r.category.toLowerCase().includes(query.toLowerCase()),
      )
    : ALL_RESULTS;

  useEffect(() => {
    if (isOpen) {
      setQuery('');
      setSelectedIndex(0);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  }, [isOpen]);

  const handleSelect = useCallback(
    (result: SearchResult) => {
      navigate(result.path);
      onClose();
    },
    [navigate, onClose],
  );

  const handleKeyDown = (e: React.KeyboardEvent) => {
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        setSelectedIndex((i) => Math.min(i + 1, filtered.length - 1));
        break;
      case 'ArrowUp':
        e.preventDefault();
        setSelectedIndex((i) => Math.max(i - 1, 0));
        break;
      case 'Enter':
        e.preventDefault();
        if (filtered[selectedIndex]) handleSelect(filtered[selectedIndex]);
        break;
      case 'Escape':
        e.preventDefault();
        onClose();
        break;
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-[100] flex items-start justify-center pt-[15vh]" onClick={onClose} role="dialog" aria-modal="true" aria-label="Search pages">
      {/* Backdrop */}
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" />

      {/* Modal */}
      <div
        className="relative w-full max-w-lg mx-4 bg-slate-900 border border-slate-700 rounded-2xl shadow-2xl animate-scale-in overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Search input */}
        <div className="flex items-center gap-3 px-5 py-4 border-b border-slate-700">
          <Search className="w-5 h-5 text-slate-400" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelectedIndex(0);
            }}
            onKeyDown={handleKeyDown}
            className="flex-1 bg-transparent text-white placeholder-slate-500 focus:outline-none text-sm"
            placeholder="Search pages, features..."
          />
          <button onClick={onClose} className="p-1.5 rounded-lg hover:bg-slate-800 text-slate-400 hover:text-slate-200 transition-colors">
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Results */}
        <div className="max-h-80 overflow-y-auto py-2">
          {filtered.length === 0 ? (
            <div className="px-5 py-8 text-center text-slate-500 text-sm">
              No results found for "{query}"
            </div>
          ) : (
            filtered.map((result, i) => (
              <button
                key={result.path}
                className={`w-full flex items-center gap-3 px-5 py-2.5 text-left transition-colors ${
                  i === selectedIndex ? 'bg-slate-800 text-white' : 'text-slate-300 hover:bg-slate-800/50 hover:text-slate-100'
                }`}
                onClick={() => handleSelect(result)}
                onMouseEnter={() => setSelectedIndex(i)}
              >
                <span className="text-slate-500">{result.icon}</span>
                <span className="flex-1 text-sm">{result.label}</span>
                <span className="text-[10px] font-medium px-2 py-0.5 rounded-full bg-slate-700/50 text-slate-400">{result.category}</span>
                <ArrowRight className="w-3 h-3 text-slate-600" />
              </button>
            ))
          )}
        </div>

        {/* Footer */}
        <div className="px-5 py-3 border-t border-slate-800 bg-slate-900/50 flex items-center gap-4 text-xs text-slate-500">
          <span>
            <kbd className="inline-flex items-center justify-center min-w-[20px] px-1.5 py-0.5 bg-slate-800 border border-slate-600 rounded text-slate-400">&uarr;&darr;</kbd> navigate
          </span>
          <span>
            <kbd className="inline-flex items-center justify-center min-w-[20px] px-1.5 py-0.5 bg-slate-800 border border-slate-600 rounded text-slate-400">Enter</kbd> select
          </span>
          <span>
            <kbd className="inline-flex items-center justify-center min-w-[20px] px-1.5 py-0.5 bg-slate-800 border border-slate-600 rounded text-slate-400">Esc</kbd> close
          </span>
        </div>
      </div>
    </div>
  );
};

export default GlobalSearch;
