// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';
import { Wifi, WifiOff } from 'lucide-react';

interface LiveBadgeProps {
  connected: boolean;
  label?: string;
  showBanner?: boolean;
}

const LiveBadge: React.FC<LiveBadgeProps> = ({ connected, label, showBanner }) => {
  if (showBanner) {
    if (connected) {
      return (
        <div className="stat-card-green rounded-xl border border-slate-700/50 px-5 py-3 flex items-center justify-between">
          <div className="flex items-center gap-3">
            <span className="w-2.5 h-2.5 rounded-full bg-emerald-500 animate-pulse" />
            <Wifi className="w-4 h-4 text-emerald-400" />
            <span className="text-sm font-medium text-emerald-400">{label ?? 'Connected'}</span>
          </div>
        </div>
      );
    }
    return (
      <div className="bg-red-500/10 rounded-xl border border-red-500/30 p-4">
        <div className="flex items-center gap-3">
          <WifiOff className="w-5 h-5 text-red-400 flex-shrink-0" />
          <p className="text-sm font-semibold text-red-400">
            {label ?? 'Disconnected - Reconnecting...'}
          </p>
        </div>
      </div>
    );
  }

  if (connected) {
    return (
      <span className="flex items-center gap-1.5 text-sm text-emerald-400">
        <span className="w-2.5 h-2.5 rounded-full bg-emerald-500 animate-pulse-dot shadow-emerald-400/50 shadow-sm" />
        <Wifi className="w-4 h-4" />
        {label ?? 'Live'}
      </span>
    );
  }
  return (
    <span className="flex items-center gap-1.5 text-sm text-slate-400">
      <span className="w-2.5 h-2.5 rounded-full bg-red-400 shadow-red-400/50 shadow-sm" />
      <WifiOff className="w-4 h-4" />
      {label ?? 'Disconnected'}
    </span>
  );
};

export default LiveBadge;
