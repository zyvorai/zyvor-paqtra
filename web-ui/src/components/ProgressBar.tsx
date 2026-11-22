// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';

const STATUS_COLORS: Record<string, string> = {
  completed: 'bg-green-400',
  running: 'bg-blue-400',
  pending: 'bg-slate-400',
  failed: 'bg-red-400',
  cancelled: 'bg-yellow-400',
  healthy: 'bg-green-400',
  degraded: 'bg-yellow-400',
  unhealthy: 'bg-red-400',
  warning: 'bg-yellow-400',
  met: 'bg-green-400',
  at_risk: 'bg-yellow-400',
  breached: 'bg-red-400',
};

interface ProgressBarProps {
  value: number;
  max?: number;
  status?: string;
  color?: string;
  showLabel?: boolean;
  size?: 'sm' | 'md' | 'lg';
}

const SIZES = { sm: 'h-1', md: 'h-1.5', lg: 'h-2' };

const ProgressBar: React.FC<ProgressBarProps> = ({
  value,
  max = 100,
  status,
  color,
  showLabel,
  size = 'md',
}) => {
  const pct = max > 0 ? Math.min((value / max) * 100, 100) : 0;
  const barColor = color || (status ? STATUS_COLORS[status] : undefined) || 'bg-blue-400';

  return (
    <div className="flex items-center gap-2">
      <div className={`flex-1 ${SIZES[size]} rounded-full bg-slate-700 overflow-hidden`}>
        <div
          className={`h-full rounded-full ${barColor} transition-all duration-300`}
          style={{ width: `${pct}%` }}
        />
      </div>
      {showLabel && (
        <span className="text-xs text-slate-500 w-10 text-right">{pct.toFixed(0)}%</span>
      )}
    </div>
  );
};

export default ProgressBar;
