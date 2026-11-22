// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useEffect } from 'react';
import { RefreshCw } from 'lucide-react';

interface DataFreshnessProps {
  lastUpdated: Date | null;
  onRefresh: () => void;
  refreshing?: boolean;
  autoRefresh?: boolean;
  onAutoRefreshToggle?: () => void;
  intervalSecs?: number;
}

function formatAge(date: Date): string {
  const diffMs = Date.now() - date.getTime();
  const secs = Math.floor(diffMs / 1000);
  if (secs < 5) return 'just now';
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  return `${hrs}h ago`;
}

function freshnessColor(date: Date): string {
  const diffSecs = (Date.now() - date.getTime()) / 1000;
  if (diffSecs <= 30) return 'text-green-400';
  if (diffSecs <= 60) return 'text-yellow-400';
  return 'text-red-400';
}

const DataFreshness: React.FC<DataFreshnessProps> = ({
  lastUpdated,
  onRefresh,
  refreshing = false,
  autoRefresh,
  onAutoRefreshToggle,
  intervalSecs,
}) => {
  // Re-render every second to keep the age text current
  const [, setTick] = useState(0);
  useEffect(() => {
    const timer = setInterval(() => setTick((t) => t + 1), 5000);
    return () => clearInterval(timer);
  }, []);

  return (
    <div className="flex items-center gap-3 text-sm">
      {lastUpdated ? (
        <span className={freshnessColor(lastUpdated)}>
          Updated {formatAge(lastUpdated)}
        </span>
      ) : (
        <span className="text-slate-500">Not yet loaded</span>
      )}

      <button
        type="button"
        onClick={onRefresh}
        disabled={refreshing}
        className="btn-refresh inline-flex items-center gap-1.5 !min-h-0 !py-1.5 !px-3 !text-sm"
        aria-label="Refresh data"
      >
        <RefreshCw className={`w-3.5 h-3.5 ${refreshing ? 'animate-spin' : ''}`} />
        Refresh
      </button>

      {onAutoRefreshToggle !== undefined && (
        <label className="flex items-center gap-2 cursor-pointer select-none">
          <button
            type="button"
            role="switch"
            aria-checked={autoRefresh}
            onClick={onAutoRefreshToggle}
            className={`relative w-9 h-5 rounded-full transition-colors ${
              autoRefresh ? 'bg-[var(--apple-blue)]' : 'bg-slate-600'
            }`}
          >
            <span
              className={`absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white transition-transform shadow-sm ${
                autoRefresh ? 'translate-x-4' : 'translate-x-0'
              }`}
            />
          </button>
          <span className="text-xs text-slate-400">
            Auto{intervalSecs ? ` (${intervalSecs}s)` : ''}
          </span>
        </label>
      )}
    </div>
  );
};

export default DataFreshness;
