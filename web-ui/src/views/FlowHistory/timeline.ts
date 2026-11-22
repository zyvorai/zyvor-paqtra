// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import type { TimelineBucketData } from '../../services/api';

/** Stack order, bottom to top. Dropped sits on the baseline, where it is easiest to compare. */
export const STACK = ['dropped', 'forwarded', 'other'] as const;
export type SeriesKey = (typeof STACK)[number];

/**
 * Legend order and color slot. Color follows the entity: forwarded is always
 * slot 1, dropped slot 2, other slot 3, whatever else is on screen.
 */
export const SERIES: { key: SeriesKey; label: string; cssVar: string }[] = [
  { key: 'forwarded', label: 'Forwarded', cssVar: '--series-1' },
  { key: 'dropped', label: 'Dropped', cssVar: '--series-2' },
  { key: 'other', label: 'Other verdicts', cssVar: '--series-3' },
];

export const MAX_BAR_WIDTH = 24;
/** Gap in the surface color between touching marks. */
export const GAP = 2;
/** A non-zero segment is never thinner than this, so a lone drop cannot vanish. */
export const MIN_SEGMENT = 2;

export const formatCount = (n: number): string => n.toLocaleString('en-US');

export const bucketTotal = (b: TimelineBucketData): number => b.forwarded + b.dropped + b.other;

/** Round a maximum up to a tidy axis top and return ticks from 0 to it. */
export function niceTicks(max: number, target = 4): number[] {
  if (!(max > 0)) return [0, 1];
  const rough = max / target;
  const pow = 10 ** Math.floor(Math.log10(rough));
  const norm = rough / pow;
  // Counts are whole numbers: a step below 1 would round to duplicate ticks.
  const step = Math.max(1, (norm <= 1 ? 1 : norm <= 2 ? 2 : norm <= 5 ? 5 : 10) * pow);
  const top = Math.ceil(max / step) * step;
  const ticks: number[] = [];
  for (let v = 0; v <= top + step / 1e6; v += step) ticks.push(Math.round(v));
  return ticks;
}

/** Indexes of up to `target` evenly spaced buckets to label on the x axis. */
export function xTickIndexes(n: number, target = 6): number[] {
  if (n <= 0) return [];
  if (n <= target) return Array.from({ length: n }, (_, i) => i);
  const step = (n - 1) / (target - 1);
  const out = new Set<number>();
  for (let i = 0; i < target; i++) out.add(Math.round(i * step));
  return [...out];
}

export interface Segment {
  key: SeriesKey;
  y: number;
  h: number;
  value: number;
}

export interface BarLayout {
  index: number;
  /** Left edge of the whole slot: the hover and focus target spans this. */
  slotX: number;
  slotW: number;
  /** The visible bar, centered in the slot and capped at MAX_BAR_WIDTH. */
  barX: number;
  barW: number;
  total: number;
  segments: Segment[];
}

/**
 * Lay out stacked columns. Segments are separated by GAP (the surface shows
 * through; no stroke is drawn), heights are proportional to the axis maximum, and
 * a non-zero segment is at least MIN_SEGMENT tall.
 */
export function layoutBars(buckets: TimelineBucketData[], plotW: number, plotH: number, axisMax: number): BarLayout[] {
  const n = buckets.length;
  if (n === 0 || axisMax <= 0) return [];
  const slotW = plotW / n;
  const barW = Math.max(1, Math.min(MAX_BAR_WIDTH, slotW - GAP));
  return buckets.map((b, index) => {
    const slotX = index * slotW;
    let cursor = plotH; // bottom of the plot; y grows downward
    const segments: Segment[] = [];
    for (const key of STACK) {
      const value = b[key];
      if (value <= 0) continue;
      const scaled = (value / axisMax) * plotH;
      const gap = segments.length > 0 ? GAP : 0;
      const h = Math.max(MIN_SEGMENT, scaled - gap);
      cursor -= gap + h;
      segments.push({ key, y: cursor, h, value });
    }
    return { index, slotX, slotW, barX: slotX + (slotW - barW) / 2, barW, total: bucketTotal(b), segments };
  });
}

/** Index of the busiest bucket, or -1 if all are empty. */
export function peakIndex(buckets: TimelineBucketData[]): number {
  let best = -1;
  let max = 0;
  buckets.forEach((b, i) => {
    const t = bucketTotal(b);
    if (t > max) {
      max = t;
      best = i;
    }
  });
  return best;
}

export type RangePreset = '15m' | '1h' | '6h' | '24h' | '7d' | 'custom';

export const PRESETS: { id: Exclude<RangePreset, 'custom'>; label: string; ms: number }[] = [
  { id: '15m', label: 'Last 15 minutes', ms: 15 * 60_000 },
  { id: '1h', label: 'Last hour', ms: 60 * 60_000 },
  { id: '6h', label: 'Last 6 hours', ms: 6 * 60 * 60_000 },
  { id: '24h', label: 'Last 24 hours', ms: 24 * 60 * 60_000 },
  { id: '7d', label: 'Last 7 days', ms: 7 * 24 * 60 * 60_000 },
];

/** ISO bounds for a preset relative to `now`, or for a custom range typed as local times. */
export function rangeFor(
  preset: RangePreset,
  now: Date,
  custom: { from: string; to: string },
): { from: string; to: string } | { error: string } {
  if (preset !== 'custom') {
    const ms = PRESETS.find((p) => p.id === preset)!.ms;
    return { from: new Date(now.getTime() - ms).toISOString(), to: now.toISOString() };
  }
  const from = new Date(custom.from);
  const to = new Date(custom.to);
  if (!custom.from || !custom.to || Number.isNaN(from.getTime()) || Number.isNaN(to.getTime())) {
    return { error: 'Choose both a start and an end time.' };
  }
  if (from >= to) return { error: 'The start must be earlier than the end.' };
  if (to.getTime() - from.getTime() > 30 * 24 * 60 * 60_000) return { error: 'A range can cover at most 30 days.' };
  return { from: from.toISOString(), to: to.toISOString() };
}

const pad = (n: number) => String(n).padStart(2, '0');

/** Local-time label for an x-axis tick: time of day, plus the date when the range spans days. */
export function tickLabel(startIso: string, rangeSecs: number): string {
  const d = new Date(startIso);
  const time = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  if (rangeSecs <= 24 * 3600) return time;
  return `${d.toLocaleString('en-US', { month: 'short', day: 'numeric' })} ${time}`;
}

/** Tooltip and table label for a bucket: its start and end in local time. */
export function bucketRangeLabel(startIso: string, bucketSecs: number): string {
  const start = new Date(startIso);
  const end = new Date(start.getTime() + bucketSecs * 1000);
  const day = (d: Date) => d.toLocaleString('en-US', { month: 'short', day: 'numeric' });
  const time = (d: Date) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  const sameDay = day(start) === day(end);
  return `${day(start)} ${time(start)} – ${sameDay ? '' : `${day(end)} `}${time(end)}`;
}
