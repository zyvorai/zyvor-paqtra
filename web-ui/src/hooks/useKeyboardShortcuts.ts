// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useEffect, useRef, useCallback } from 'react';

interface ShortcutHandlers {
  onToggleHelp?: () => void;
  onToggleSearch?: () => void;
  onRefresh?: () => void;
  navigate?: (path: string) => void;
}

export interface Shortcut {
  keys: string;
  description: string;
  category: 'Navigation' | 'Actions' | 'General';
}

export const shortcuts: Shortcut[] = [
  { keys: '?', description: 'Show keyboard shortcuts', category: 'General' },
  { keys: '/', description: 'Open global search', category: 'General' },
  { keys: 'r', description: 'Refresh current view', category: 'Actions' },
  { keys: 'g h', description: 'Go to Dashboard', category: 'Navigation' },
  { keys: 'g f', description: 'Go to Flows', category: 'Navigation' },
  { keys: 'g t', description: 'Go to Topology', category: 'Navigation' },
  { keys: 'g p', description: 'Go to Policies', category: 'Navigation' },
  { keys: 'g a', description: 'Go to Anomalies', category: 'Navigation' },
  { keys: 'g c', description: 'Go to Compliance', category: 'Navigation' },
  { keys: 'g o', description: 'Go to AutoPolicy', category: 'Navigation' },
  { keys: 'g x', description: 'Go to Chaos', category: 'Navigation' },
  { keys: 'g n', description: 'Go to Canary', category: 'Navigation' },
  { keys: 'g s', description: 'Go to Settings', category: 'Navigation' },
];

const NAV_MAP: Record<string, string> = {
  h: '/',
  f: '/flows',
  t: '/topology',
  p: '/policies',
  a: '/anomalies',
  c: '/compliance',
  o: '/autopolicy',
  x: '/chaos',
  n: '/canary',
  s: '/settings',
};

export function useKeyboardShortcuts(handlers: ShortcutHandlers) {
  const gPrefixRef = useRef(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const handlersRef = useRef(handlers);
  useEffect(() => { handlersRef.current = handlers; });

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      // Skip if user is typing in an input
      const tag = (e.target as HTMLElement).tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
      if (e.ctrlKey || e.metaKey || e.altKey) return;

      const key = e.key.toLowerCase();

      if (gPrefixRef.current) {
        gPrefixRef.current = false;
        if (timerRef.current) clearTimeout(timerRef.current);
        const path = NAV_MAP[key];
        if (path) {
          e.preventDefault();
          handlersRef.current.navigate?.(path);
        }
        return;
      }

      switch (key) {
        case '?':
          e.preventDefault();
          handlersRef.current.onToggleHelp?.();
          break;
        case '/':
          e.preventDefault();
          handlersRef.current.onToggleSearch?.();
          break;
        case 'r':
          e.preventDefault();
          handlersRef.current.onRefresh?.();
          break;
        case 'g':
          e.preventDefault();
          gPrefixRef.current = true;
          timerRef.current = setTimeout(() => {
            gPrefixRef.current = false;
          }, 1000);
          break;
      }
    },
    [],
  );

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [handleKeyDown]);
}
