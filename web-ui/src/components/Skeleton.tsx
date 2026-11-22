// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';

export const SkeletonLine: React.FC<{ className?: string }> = ({ className = '' }) => (
  <div className={`h-4 rounded skeleton ${className}`} />
);

export const SkeletonCard: React.FC = () => (
  <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5 space-y-3">
    <SkeletonLine className="w-1/3 h-3" />
    <SkeletonLine className="w-2/3 h-6" />
    <SkeletonLine className="w-1/2 h-3" />
  </div>
);

export const SkeletonTable: React.FC<{ rows?: number; cols?: number }> = ({ rows = 5, cols = 4 }) => (
  <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
    <div className="border-b border-slate-700/50 bg-slate-900/50 px-5 py-4 flex gap-4">
      {Array.from({ length: cols }).map((_, i) => <SkeletonLine key={i} className="flex-1 h-3" />)}
    </div>
    {Array.from({ length: rows }).map((_, r) => (
      <div key={r} className="px-5 py-3 flex gap-4 border-b border-slate-700/30">
        {Array.from({ length: cols }).map((_, c) => <SkeletonLine key={c} className="flex-1 h-3" />)}
      </div>
    ))}
  </div>
);

export const SkeletonChart: React.FC = () => (
  <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-5">
    <SkeletonLine className="w-1/4 h-4 mb-4" />
    <div className="h-64 rounded skeleton" />
  </div>
);

const GRID_COLS: Record<number, string> = {
  2: 'lg:grid-cols-2',
  3: 'lg:grid-cols-3',
  4: 'lg:grid-cols-4',
  5: 'lg:grid-cols-5',
  6: 'lg:grid-cols-6',
};

export const SkeletonStats: React.FC<{ count?: number }> = ({ count = 4 }) => (
  <div className={`grid grid-cols-2 ${GRID_COLS[count] ?? 'lg:grid-cols-4'} gap-4`}>
    {Array.from({ length: count }).map((_, i) => <SkeletonCard key={i} />)}
  </div>
);
