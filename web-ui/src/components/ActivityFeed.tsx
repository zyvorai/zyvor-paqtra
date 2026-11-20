// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react';
import { Activity, CheckCircle, AlertTriangle, XCircle, Info } from 'lucide-react';

interface FeedEvent {
  id: string;
  type: 'info' | 'success' | 'warning' | 'error';
  message: string;
  timestamp: string;
  source?: string;
}

interface ActivityFeedProps {
  events: FeedEvent[];
  maxItems?: number;
  title?: string;
}

const EVENT_CONFIG: Record<
  FeedEvent['type'],
  { icon: React.ReactNode; dot: string; badge: string }
> = {
  info: {
    icon: <Info className="w-3.5 h-3.5 text-blue-400" />,
    dot: 'bg-blue-400',
    badge: 'bg-blue-500/10 text-blue-400',
  },
  success: {
    icon: <CheckCircle className="w-3.5 h-3.5 text-emerald-400" />,
    dot: 'bg-emerald-400',
    badge: 'bg-emerald-500/10 text-emerald-400',
  },
  warning: {
    icon: <AlertTriangle className="w-3.5 h-3.5 text-yellow-400" />,
    dot: 'bg-yellow-400',
    badge: 'bg-yellow-500/10 text-yellow-400',
  },
  error: {
    icon: <XCircle className="w-3.5 h-3.5 text-red-400" />,
    dot: 'bg-red-400',
    badge: 'bg-red-500/10 text-red-400',
  },
};

function formatRelativeTime(timestamp: string): string {
  const now = Date.now();
  const then = new Date(timestamp).getTime();
  const diffMs = now - then;

  if (Number.isNaN(diffMs)) return timestamp;

  const seconds = Math.floor(diffMs / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export const ActivityFeed: React.FC<ActivityFeedProps> = ({
  events,
  maxItems = 10,
  title = 'Recent Activity',
}) => {
  const sorted = [...events]
    .sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime())
    .slice(0, maxItems);

  return (
    <div className="bg-slate-800/50 rounded-xl border border-slate-700/50 p-5">
      <h3 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 shadow-lg shadow-blue-500/20 flex items-center justify-center">
          <Activity className="w-4 h-4 text-white" />
        </div>
        {title}
        <span className="ml-auto text-[10px] font-medium px-2 py-0.5 rounded-full bg-slate-700/50 text-slate-400">
          {events.length} events
        </span>
      </h3>

      {sorted.length === 0 ? (
        <div className="text-center py-8 text-slate-500 text-sm">
          No recent activity
        </div>
      ) : (
        <div className="max-h-[400px] overflow-y-auto space-y-1 scrollbar-thin">
          {sorted.map((event) => {
            const cfg = EVENT_CONFIG[event.type];
            return (
              <div
                key={event.id}
                className="flex items-start gap-3 rounded-lg px-3 py-2.5 transition-colors hover:bg-slate-700/20"
              >
                <div className="mt-0.5 flex-shrink-0 w-6 h-6 rounded-md bg-slate-900/50 flex items-center justify-center">
                  {cfg.icon}
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-sm text-slate-200 leading-snug">
                    {event.message}
                  </p>
                  <div className="flex items-center gap-2 mt-1">
                    <span className="text-[11px] text-slate-500">
                      {formatRelativeTime(event.timestamp)}
                    </span>
                    {event.source && (
                      <span className={`text-[10px] font-medium px-1.5 py-0.5 rounded ${cfg.badge}`}>
                        {event.source}
                      </span>
                    )}
                  </div>
                </div>
                <span className={`mt-1.5 flex-shrink-0 w-1.5 h-1.5 rounded-full ${cfg.dot}`} />
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};

export default ActivityFeed;
