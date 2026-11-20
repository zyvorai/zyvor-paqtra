// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react';
import { CheckCircle, AlertTriangle, XCircle, HelpCircle } from 'lucide-react';

interface HealthCheck {
  name: string;
  status: 'healthy' | 'degraded' | 'unhealthy' | 'unknown';
  detail?: string;
}

interface HealthCheckCardProps {
  checks: HealthCheck[];
}

const STATUS_CONFIG: Record<
  HealthCheck['status'],
  { icon: React.ReactNode; color: string; bg: string; dot: string; label: string }
> = {
  healthy: {
    icon: <CheckCircle className="w-5 h-5 text-emerald-400" />,
    color: 'text-emerald-400',
    bg: 'bg-emerald-500/10',
    dot: 'bg-emerald-400',
    label: 'Healthy',
  },
  degraded: {
    icon: <AlertTriangle className="w-5 h-5 text-yellow-400" />,
    color: 'text-yellow-400',
    bg: 'bg-yellow-500/10',
    dot: 'bg-yellow-400',
    label: 'Degraded',
  },
  unhealthy: {
    icon: <XCircle className="w-5 h-5 text-red-400" />,
    color: 'text-red-400',
    bg: 'bg-red-500/10',
    dot: 'bg-red-400',
    label: 'Unhealthy',
  },
  unknown: {
    icon: <HelpCircle className="w-5 h-5 text-slate-400" />,
    color: 'text-slate-400',
    bg: 'bg-slate-500/10',
    dot: 'bg-slate-400',
    label: 'Unknown',
  },
};

export const HealthCheckCard: React.FC<HealthCheckCardProps> = ({ checks }) => {
  return (
    <div className="bg-slate-800/50 rounded-xl border border-slate-700/50 p-5">
      <h3 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-emerald-500 to-emerald-700 shadow-lg shadow-emerald-500/20 flex items-center justify-center">
          <CheckCircle className="w-4 h-4 text-white" />
        </div>
        Health Checks
      </h3>
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
        {checks.map((check) => {
          const cfg = STATUS_CONFIG[check.status];
          return (
            <div
              key={check.name}
              className="bg-slate-900/50 rounded-lg p-3 border border-slate-700/30 transition-all hover:border-slate-600/50"
            >
              <div className="flex items-start gap-3">
                <div className={`mt-0.5 w-8 h-8 rounded-lg ${cfg.bg} flex items-center justify-center flex-shrink-0`}>
                  {cfg.icon}
                </div>
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium text-white truncate">
                    {check.name}
                  </div>
                  <div className="flex items-center gap-1.5 mt-1">
                    <span className="relative flex h-2 w-2">
                      {check.status === 'healthy' && (
                        <span className={`animate-ping absolute inline-flex h-full w-full rounded-full ${cfg.dot} opacity-75`} />
                      )}
                      <span className={`relative inline-flex rounded-full h-2 w-2 ${cfg.dot}`} />
                    </span>
                    <span className={`text-xs font-medium ${cfg.color}`}>
                      {cfg.label}
                    </span>
                  </div>
                  {check.detail && (
                    <div className="text-xs text-slate-500 mt-1.5 truncate">
                      {check.detail}
                    </div>
                  )}
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
};

export default HealthCheckCard;
