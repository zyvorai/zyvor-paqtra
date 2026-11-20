// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react';

interface ScoreGaugeProps {
  score: number;
  max?: number;
  size?: number;
  label?: string;
}

function scoreColor(score: number): string {
  if (score >= 80) return 'var(--accent-green)';
  if (score >= 60) return 'var(--accent-amber)';
  return 'var(--danger)';
}

const ScoreGauge: React.FC<ScoreGaugeProps> = ({ score, max = 100, size = 120, label }) => {
  const pct = max > 0 ? Math.min(score / max, 1) : 0;
  const r = (size / 2) - 8;
  const circumference = 2 * Math.PI * r;

  return (
    <div className="relative inline-flex items-center justify-center" style={{ width: size, height: size }}>
      <svg className="w-full h-full -rotate-90" viewBox={`0 0 ${size} ${size}`}>
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke="var(--border)"
          strokeWidth="8"
        />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke={scoreColor(score)}
          strokeWidth="8"
          strokeDasharray={`${pct * circumference} ${circumference}`}
          strokeLinecap="round"
          className="transition-all duration-500"
        />
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center">
        <span className="text-3xl font-bold" style={{ color: scoreColor(score) }}>
          {Math.round(score)}
        </span>
        {label && <span className="text-xs" style={{ color: 'var(--text-tertiary)' }}>{label}</span>}
      </div>
    </div>
  );
};

export default ScoreGauge;
