// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState, useCallback } from 'react';

interface UseMetricsHistoryReturn<T> {
  history: T[];
  addMetrics: (metrics: T) => void;
  clearHistory: () => void;
}

export function useMetricsHistory<T = Record<string, unknown>>(maxPoints = 100): UseMetricsHistoryReturn<T> {
  const [history, setHistory] = useState<T[]>([]);

  const addMetrics = useCallback(
    (metrics: T) => {
      setHistory((prev) => {
        const updated = [...prev, metrics];
        return updated.slice(-maxPoints);
      });
    },
    [maxPoints]
  );

  const clearHistory = useCallback(() => {
    setHistory([]);
  }, []);

  return { history, addMetrics, clearHistory };
}
