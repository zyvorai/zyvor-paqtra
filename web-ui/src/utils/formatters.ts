// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Shared formatting utilities — single source of truth */

export function formatBytes(bytes: number | undefined | null): string {
  if (bytes === null || bytes === undefined || bytes === 0) return '0 B';
  if (bytes < 0) return '-' + formatBytes(-bytes);
  if (bytes >= 1e12) return `${(bytes / 1e12).toFixed(1)} TB`;
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(1)} MB`;
  if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(1)} KB`;
  return `${bytes} B`;
}

export function formatDuration(seconds: number | undefined | null): string {
  if (!seconds || seconds <= 0) return '0s';
  const d = Math.floor(seconds / 86400);
  const h = Math.floor((seconds % 86400) / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

export function formatRelativeTime(dateStr: string | undefined | null): string {
  if (!dateStr) return '-';
  try {
    const diff = Date.now() - new Date(dateStr).getTime();
    if (diff < 0) return 'just now';
    const secs = Math.floor(diff / 1000);
    if (secs < 60) return `${secs}s ago`;
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.floor(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.floor(hours / 24);
    return `${days}d ago`;
  } catch {
    return dateStr;
  }
}

export function formatNumber(n: number | undefined | null): string {
  if (n == null || isNaN(n)) return '0';
  if (n >= 1e6) return `${(n / 1e6).toFixed(1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`;
  return n.toLocaleString();
}

export function formatCount(n: number | undefined | null): string {
  if (n == null || isNaN(n)) return '0';
  return n.toLocaleString();
}

export function getStatusColor(status: string): string {
  const map: Record<string, string> = {
    healthy: 'text-green-400', ready: 'text-green-400', active: 'text-green-400',
    connected: 'text-green-400', ok: 'text-green-400', running: 'text-green-400',
    pass: 'text-green-400', compliant: 'text-green-400', met: 'text-green-400',
    degraded: 'text-yellow-400', warning: 'text-yellow-400', pending: 'text-yellow-400',
    partial: 'text-yellow-400', at_risk: 'text-yellow-400',
    unhealthy: 'text-red-400', error: 'text-red-400', failed: 'text-red-400',
    critical: 'text-red-400', disconnected: 'text-red-400', breached: 'text-red-400',
  };
  return map[status.toLowerCase()] ?? 'text-muted-foreground';
}

export function getStatusHexColor(status: string): string {
  const map: Record<string, string> = {
    pending: '#6b7280',
    running: '#3b82f6',
    completed: '#10b981',
    failed: '#ef4444',
    cancelled: '#f59e0b',
    healthy: '#10b981',
    degraded: '#f59e0b',
    unhealthy: '#ef4444',
  };
  return map[status] || '#6b7280';
}

export function getStatusIcon(status: string): string {
  const icons: Record<string, string> = {
    pending: '\u23F3',
    running: '\u25B6\uFE0F',
    completed: '\u2705',
    failed: '\u274C',
    cancelled: '\uD83D\uDEAB',
    healthy: '\u2705',
    degraded: '\u26A0\uFE0F',
    unhealthy: '\u274C',
  };
  return icons[status] || '\u25CF';
}

export function getSeverityColor(severity: string): string {
  const map: Record<string, string> = {
    info: '#3b82f6',
    warning: '#f59e0b',
    error: '#ef4444',
    critical: '#991b1b',
  };
  return map[severity] || '#6b7280';
}

export function formatTimestamp(timestamp: string | undefined | null): string {
  if (!timestamp) return '-';
  try {
    return new Date(timestamp).toLocaleString();
  } catch {
    return timestamp;
  }
}

export function formatPercentage(value: number | undefined | null, decimals = 1): string {
  if (value == null || isNaN(value)) return '0%';
  return `${value.toFixed(decimals)}%`;
}
