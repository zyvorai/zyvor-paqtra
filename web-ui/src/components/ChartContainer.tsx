// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react';
import { BarChart3 } from 'lucide-react';
import {
  LineChart,
  Line,
  BarChart,
  Bar,
  PieChart,
  Pie,
  Cell,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from 'recharts';

interface ChartContainerProps {
  title: string;
  type: 'line' | 'bar' | 'pie';
  data: Record<string, unknown>[];
  dataKeys?: string[];
  /** Friendly legend name per data key; falls back to the raw key. */
  dataKeyLabels?: Record<string, string>;
  colors?: string[];
  xAxisKey?: string;
  height?: number;
  icon?: React.ReactNode;
  /** Formats Y-axis ticks and tooltip values, e.g. `(v) => `$${v}``. */
  valueFormatter?: (value: number) => string;
}

const DEFAULT_COLORS = [
  'var(--apple-blue)',
  'var(--accent-green)',
  'var(--accent-amber)',
  'var(--danger)',
  'var(--accent-purple)',
  'var(--accent-cyan)',
];

const tooltipStyle = {
  backgroundColor: 'var(--bg-elevated)',
  border: '1px solid var(--border)',
  borderRadius: 'var(--radius-md)',
  fontSize: '12px',
  color: 'var(--text-primary)',
};

export const ChartContainer: React.FC<ChartContainerProps> = ({
  title,
  type,
  data,
  dataKeys = [],
  dataKeyLabels = {},
  colors = DEFAULT_COLORS,
  xAxisKey = 'timestamp',
  height = 300,
  icon,
  valueFormatter,
}) => {
  const renderChart = () => {
    switch (type) {
      case 'line':
        return (
          <ResponsiveContainer width="100%" height={height}>
            <LineChart data={data}>
              <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
              <XAxis
                dataKey={xAxisKey}
                stroke="var(--text-tertiary)"
                fontSize={11}
                tickFormatter={(value) => {
                  if (xAxisKey === 'timestamp') {
                    const date = new Date(value);
                    return date.toLocaleTimeString();
                  }
                  return value;
                }}
              />
              <YAxis stroke="var(--text-tertiary)" fontSize={11} tickFormatter={valueFormatter} />
              <Tooltip
                contentStyle={tooltipStyle}
                formatter={valueFormatter ? (v) => valueFormatter(Number(v ?? 0)) : undefined}
              />
              <Legend />
              {dataKeys.map((key, index) => (
                <Line
                  key={key}
                  type="monotone"
                  dataKey={key}
                  name={dataKeyLabels[key] ?? key}
                  stroke={colors[index % colors.length]}
                  strokeWidth={2}
                  dot={false}
                  animationDuration={300}
                />
              ))}
            </LineChart>
          </ResponsiveContainer>
        );

      case 'bar':
        return (
          <ResponsiveContainer width="100%" height={height}>
            <BarChart data={data}>
              <CartesianGrid strokeDasharray="3 3" stroke="var(--border)" />
              <XAxis dataKey={xAxisKey} stroke="var(--text-tertiary)" fontSize={11} />
              <YAxis stroke="var(--text-tertiary)" fontSize={11} tickFormatter={valueFormatter} />
              <Tooltip
                contentStyle={tooltipStyle}
                formatter={valueFormatter ? (v) => valueFormatter(Number(v ?? 0)) : undefined}
              />
              <Legend />
              {dataKeys.map((key, index) => (
                <Bar
                  key={key}
                  dataKey={key}
                  name={dataKeyLabels[key] ?? key}
                  fill={colors[index % colors.length]}
                  radius={[2, 2, 0, 0]}
                  animationDuration={300}
                />
              ))}
            </BarChart>
          </ResponsiveContainer>
        );

      case 'pie':
        return (
          <ResponsiveContainer width="100%" height={height}>
            <PieChart>
              <Pie
                data={data}
                dataKey="value"
                nameKey="name"
                cx="50%"
                cy="50%"
                outerRadius={80}
                label
                animationDuration={300}
              >
                {data.map((_, index) => (
                  <Cell key={`cell-${index}`} fill={colors[index % colors.length]} />
                ))}
              </Pie>
              <Tooltip contentStyle={tooltipStyle} />
              <Legend />
            </PieChart>
          </ResponsiveContainer>
        );

      default:
        return null;
    }
  };

  return (
    <div className="card">
      <h3 className="text-sm font-semibold flex items-center gap-2 mb-4" style={{ color: 'var(--text-primary)' }}>
        {icon || <BarChart3 className="w-4 h-4" style={{ color: 'var(--text-tertiary)' }} />}
        {title}
      </h3>
      {data.length === 0 ? (
        <div
          className="flex items-center justify-center"
          style={{ height, color: 'var(--text-tertiary)' }}
        >
          No data available
        </div>
      ) : (
        renderChart()
      )}
    </div>
  );
};

export default ChartContainer;
