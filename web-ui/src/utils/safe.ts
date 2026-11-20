// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/** Safe number formatting — prevents crashes on undefined/null API responses */

export function safeFixed(val: number | undefined | null, digits = 1): string {
  if (val == null || isNaN(val)) return (0).toFixed(digits);
  return val.toFixed(digits);
}

export function safeLocale(val: number | undefined | null): string {
  if (val == null || isNaN(val)) return '0';
  return val.toLocaleString();
}

export function safePct(used: number | undefined | null, total: number | undefined | null): number {
  if (!used || !total || total === 0) return 0;
  return Math.min((used / total) * 100, 100);
}

export function safeArray<T>(arr: T[] | undefined | null): T[] {
  return arr ?? [];
}
