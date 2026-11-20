// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react';
import { X, Loader2 } from 'lucide-react';

interface BulkAction {
  label: string;
  icon: React.ReactNode;
  onClick: () => void;
  variant?: 'default' | 'danger';
  loading?: boolean;
}

interface BulkActionBarProps {
  selectedCount: number;
  onClear: () => void;
  actions: BulkAction[];
}

const BulkActionBar: React.FC<BulkActionBarProps> = ({
  selectedCount,
  onClear,
  actions,
}) => {
  if (selectedCount <= 0) return null;

  return (
    <div className="animate-slide-in-up sticky bottom-4 z-30 mx-4">
      <div className="flex items-center gap-4 px-5 py-3 rounded-xl border border-slate-600/50 bg-slate-800/95 backdrop-blur-sm shadow-xl">
        <span className="text-sm font-medium text-slate-200">
          <span className="inline-flex items-center justify-center min-w-[1.5rem] h-6 px-1.5 rounded-full bg-blue-600 text-white text-xs font-bold mr-2">
            {selectedCount}
          </span>
          selected
        </span>

        <button
          onClick={onClear}
          className="p-1.5 rounded-lg text-slate-400 hover:text-white hover:bg-slate-700/50 transition-colors"
          aria-label="Clear selection"
        >
          <X className="w-4 h-4" />
        </button>

        <div className="w-px h-6 bg-slate-700/50" />

        <div className="flex items-center gap-2">
          {actions.map((action) => {
            const isDanger = action.variant === 'danger';
            return (
              <button
                key={action.label}
                onClick={action.onClick}
                disabled={action.loading}
                className={`inline-flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed ${
                  isDanger
                    ? 'text-red-400 hover:bg-red-500/20 hover:text-red-300'
                    : 'text-slate-300 hover:bg-slate-700/50 hover:text-white'
                }`}
              >
                {action.loading ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  action.icon
                )}
                {action.label}
              </button>
            );
          })}
        </div>
      </div>

    </div>
  );
};

export default BulkActionBar;
