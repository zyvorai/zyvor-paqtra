// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';

const VARIANT_MAP: Record<string, string> = {
  // Status
  completed: 'bg-green-500/20 text-green-400 border-green-500/30',
  running: 'bg-blue-500/20 text-blue-400 border-blue-500/30',
  active: 'bg-blue-500/20 text-blue-400 border-blue-500/30',
  pending: 'bg-slate-500/20 text-slate-400 border-slate-500/30',
  failed: 'bg-red-500/20 text-red-400 border-red-500/30',
  error: 'bg-red-500/20 text-red-400 border-red-500/30',
  cancelled: 'bg-yellow-500/20 text-yellow-400 border-yellow-500/30',
  warning: 'bg-yellow-500/20 text-yellow-400 border-yellow-500/30',
  healthy: 'bg-green-500/20 text-green-400 border-green-500/30',
  degraded: 'bg-yellow-500/20 text-yellow-400 border-yellow-500/30',
  unhealthy: 'bg-red-500/20 text-red-400 border-red-500/30',
  // Severity
  info: 'bg-blue-500/20 text-blue-400 border-blue-500/30',
  critical: 'bg-red-500/20 text-red-400 border-red-500/30',
  // Verdict
  FORWARDED: 'bg-green-500/15 text-green-400 border-green-500/30',
  DROPPED: 'bg-red-500/15 text-red-400 border-red-500/30',
  AUDIT: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30',
  // Roles
  admin: 'bg-purple-500/20 text-purple-400 border-purple-500/30',
  editor: 'bg-blue-500/20 text-blue-400 border-blue-500/30',
  viewer: 'bg-slate-500/20 text-slate-400 border-slate-500/30',
  // Generic
  default: 'bg-slate-500/20 text-slate-400 border-slate-500/30',
};

interface BadgeProps {
  variant?: string;
  children: React.ReactNode;
  icon?: React.ReactNode;
  className?: string;
}

const Badge: React.FC<BadgeProps> = ({ variant = 'default', children, icon, className = '' }) => {
  const colors = VARIANT_MAP[variant] || VARIANT_MAP.default;
  return (
    <span className={`inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-medium border ${colors} ${className}`}>
      {icon}
      {children}
    </span>
  );
};

export default Badge;
