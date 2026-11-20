// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useCallback, useState } from 'react';

/**
 * Hook that provides a state setter which auto-clears after a timeout.
 * Usage: const [success, setSuccess] = useAutoDismiss<string | null>(null, 4000);
 */
export function useAutoDismiss<T>(initial: T, ms = 4000): [T, (val: T) => void] {
  const [value, setValue] = useState<T>(initial);

  useEffect(() => {
    if (value == null || value === '') return;
    const timer = setTimeout(() => setValue(initial), ms);
    return () => clearTimeout(timer);
  }, [value, initial, ms]);

  const set = useCallback((val: T) => setValue(val), []);

  return [value, set];
}
