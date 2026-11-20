// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react';
import { AlertTriangle, RefreshCw, X } from 'lucide-react';

interface ErrorRetryProps {
  message: string;
  onRetry?: () => void;
  onDismiss?: () => void;
}

const ErrorRetry: React.FC<ErrorRetryProps> = ({ message, onRetry, onDismiss }) => (
  <div className="mb-4 p-4 rounded-xl bg-red-500/10 border border-red-500/30 flex items-start gap-3">
    <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-red-500 to-red-700 flex items-center justify-center shadow-lg shadow-red-500/20 flex-shrink-0">
      <AlertTriangle className="w-4 h-4 text-white" />
    </div>
    <div className="flex-1">
      <p className="text-sm font-semibold text-red-400">{message}</p>
    </div>
    <div className="flex items-center gap-2">
      {onRetry && (
        <button
          onClick={onRetry}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-gradient-to-r from-slate-700 to-slate-800 text-slate-300 hover:text-white text-xs shadow-sm transition-colors"
        >
          <RefreshCw className="w-3 h-3" /> Retry
        </button>
      )}
      {onDismiss && (
        <button
          onClick={onDismiss}
          className="p-1.5 rounded-lg hover:bg-slate-800 text-slate-400 hover:text-slate-200 transition-colors"
        >
          <X className="w-4 h-4" />
        </button>
      )}
    </div>
  </div>
);

export default ErrorRetry;
