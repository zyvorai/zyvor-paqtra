// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useState, useCallback, useMemo } from 'react';

interface UsePaginationOptions {
  initialPage?: number;
  initialLimit?: number;
  total?: number;
}

interface UsePaginationReturn {
  page: number;
  limit: number;
  offset: number;
  total: number;
  totalPages: number;
  setTotal: (total: number) => void;
  nextPage: () => void;
  prevPage: () => void;
  goToPage: (page: number) => void;
  setLimit: (limit: number) => void;
  resetPage: () => void;
  hasNextPage: boolean;
  hasPrevPage: boolean;
  pageRange: { start: number; end: number };
}

export function usePagination(options?: UsePaginationOptions): UsePaginationReturn {
  const initialPage = options?.initialPage ?? 0;
  const initialLimit = options?.initialLimit ?? 25;
  const initialTotal = options?.total ?? 0;

  const [page, setPage] = useState(initialPage);
  const [limit, setLimitState] = useState(initialLimit);
  const [total, setTotal] = useState(initialTotal);

  const offset = page * limit;

  const totalPages = useMemo(
    () => (total > 0 ? Math.ceil(total / limit) : 0),
    [total, limit],
  );

  const hasNextPage = total > 0 && (page + 1) * limit < total;
  const hasPrevPage = page > 0;

  const pageRange = useMemo(() => {
    const start = total > 0 ? offset + 1 : 0;
    const end = Math.min(offset + limit, total);
    return { start, end };
  }, [offset, limit, total]);

  const nextPage = useCallback(() => {
    setPage((p) => {
      const maxPage = Math.max(0, Math.ceil(total / limit) - 1);
      return Math.min(p + 1, maxPage);
    });
  }, [total, limit]);

  const prevPage = useCallback(() => {
    setPage((p) => Math.max(0, p - 1));
  }, []);

  const goToPage = useCallback(
    (target: number) => {
      const maxPage = total > 0 ? Math.ceil(total / limit) - 1 : 0;
      setPage(Math.max(0, Math.min(target, maxPage)));
    },
    [total, limit],
  );

  const setLimit = useCallback((newLimit: number) => {
    setLimitState(newLimit);
    setPage(0);
  }, []);

  const resetPage = useCallback(() => {
    setPage(0);
  }, []);

  return {
    page,
    limit,
    offset,
    total,
    totalPages,
    setTotal,
    nextPage,
    prevPage,
    goToPage,
    setLimit,
    resetPage,
    hasNextPage,
    hasPrevPage,
    pageRange,
  };
}
