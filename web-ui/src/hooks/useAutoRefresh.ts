// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useEffect, useRef, useState, useCallback } from 'react';

interface UseAutoRefreshResult {
  lastUpdated: Date | null;
  refreshing: boolean;
  manualRefresh: () => void;
}

export function useAutoRefresh(
  fetchFn: () => Promise<void>,
  intervalMs: number,
  enabled: boolean,
  initialFetch: boolean = true,
): UseAutoRefreshResult {
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const fetchRef = useRef(fetchFn);
  const mountedRef = useRef(true);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Keep fetch ref current without re-triggering the effect
  useEffect(() => {
    fetchRef.current = fetchFn;
  }, [fetchFn]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const doFetch = useCallback(async () => {
    if (!mountedRef.current) return;
    setRefreshing(true);
    try {
      await fetchRef.current();
      if (mountedRef.current) {
        setLastUpdated(new Date());
      }
    } finally {
      if (mountedRef.current) {
        setRefreshing(false);
      }
    }
  }, []);

  // Fetch on mount if initialFetch is true (default). The user typically
  // expects data immediately. The `enabled` flag only controls the recurring interval.
  useEffect(() => {
    if (initialFetch) {
      doFetch();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [doFetch]);

  // Interval-based refresh when enabled
  useEffect(() => {
    if (!enabled || intervalMs <= 0) {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
      return;
    }

    intervalRef.current = setInterval(() => {
      doFetch();
    }, intervalMs);

    return () => {
      if (intervalRef.current) {
        clearInterval(intervalRef.current);
        intervalRef.current = null;
      }
    };
  }, [enabled, intervalMs, doFetch]);

  const manualRefresh = useCallback(() => {
    doFetch();
  }, [doFetch]);

  return { lastUpdated, refreshing, manualRefresh };
}
