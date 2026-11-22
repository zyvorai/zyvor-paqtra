// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';
import { TrendingUp, TrendingDown } from 'lucide-react';

interface StatCardProps {
  title: string;
  value: string | number;
  subtitle?: string;
  icon?: React.ReactNode;
  color?: string;
  badge?: string;
  trend?: {
    value: number;
    isPositive: boolean;
  };
}

export function StatCard({
  title,
  value,
  subtitle,
  icon,
  badge,
  trend,
}: StatCardProps) {
  return (
    <div className="card metrics">
      <div className="metric">
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 8 }}>
          {icon ? <span aria-hidden>{icon}</span> : <span />}
          {badge ? <span className="eyebrow">{badge}</span> : null}
        </div>
        <div className="metric-value">{value}</div>
        <div className="metric-label">{title}</div>
        {subtitle ? <p style={{ margin: '4px 0 0', color: 'var(--text-tertiary)', fontSize: 12 }}>{subtitle}</p> : null}
        {trend ? (
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 4,
              marginTop: 6,
              fontSize: 12,
              fontWeight: 600,
              color: trend.isPositive ? 'var(--accent-green)' : 'var(--danger)',
            }}
          >
            {trend.isPositive ? <TrendingUp size={12} /> : <TrendingDown size={12} />}
            {Math.abs(trend.value).toFixed(1)}%
          </div>
        ) : null}
      </div>
    </div>
  );
}

export default StatCard;
