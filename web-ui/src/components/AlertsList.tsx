// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';
import { Bell } from 'lucide-react';
import { formatRelativeTime } from '../utils/formatters';

interface Alert {
  id: string;
  severity: string;
  title: string;
  message: string;
  timestamp: string;
  acknowledged?: boolean;
}

const SEVERITY_COLORS: Record<string, string> = {
  info: '#3b82f6',
  warning: '#f59e0b',
  error: '#ef4444',
  critical: '#991b1b',
};

function getSeverityColor(severity: string): string {
  return SEVERITY_COLORS[severity] || '#6b7280';
}

interface AlertsListProps {
  alerts: Alert[];
  onDismiss?: (alertId: string) => void;
}

export const AlertsList: React.FC<AlertsListProps> = ({ alerts, onDismiss }) => {
  if (alerts.length === 0) {
    return (
      <div className="bg-slate-800/50 rounded-xl p-10 border border-slate-700/50 flex flex-col items-center justify-center text-slate-500 gap-3">
        <Bell className="w-8 h-8" />
        <span className="text-sm">No active alerts</span>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {alerts.map((alert) => {
        const severityColor = getSeverityColor(alert.severity);

        return (
          <div
            key={alert.id}
            className="bg-slate-800/50 rounded-xl p-4 border border-slate-700/50 flex justify-between items-start"
            style={{ borderLeftWidth: '4px', borderLeftColor: severityColor }}
          >
            <div className="flex-1">
              <div className="flex items-center gap-2 mb-2">
                <span
                  className="rounded-full px-2.5 py-0.5 text-xs font-medium"
                  style={{
                    backgroundColor: severityColor + '20',
                    color: severityColor,
                  }}
                >
                  {alert.severity}
                </span>
                <span className="text-xs text-slate-500">
                  {formatRelativeTime(alert.timestamp)}
                </span>
              </div>
              <div className="text-sm font-semibold text-white mb-1">
                {alert.title}
              </div>
              <div className="text-sm text-slate-400">
                {alert.message}
              </div>
            </div>
            {onDismiss && !alert.acknowledged && (
              <button
                onClick={() => onDismiss(alert.id)}
                className="ml-4 px-3 py-1 text-xs bg-gradient-to-r from-slate-700 to-slate-800 border-0 text-slate-300 hover:text-white shadow-sm rounded-lg transition-colors cursor-pointer"
              >
                Dismiss
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
};

export default AlertsList;
