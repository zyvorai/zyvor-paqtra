// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { describe, it, expect } from 'vitest';
import {
  niceTicks, xTickIndexes, layoutBars, peakIndex, rangeFor, tickLabel, bucketRangeLabel,
  bucketTotal, formatCount, GAP, MAX_BAR_WIDTH, MIN_SEGMENT, SERIES, STACK,
} from '../timeline';
import type { TimelineBucketData } from '../../../services/api';

const b = (forwarded: number, dropped: number, other = 0, start = '2026-01-01T00:00:00Z'): TimelineBucketData => ({ start, forwarded, dropped, other });
const local = (y: number, mo: number, d: number, h: number, mi: number) => new Date(y, mo - 1, d, h, mi).toISOString();

describe('niceTicks', () => {
  it('rounds the top up to a tidy number and starts at zero', () => {
    expect(niceTicks(7)).toEqual([0, 2, 4, 6, 8]);
    expect(niceTicks(100)).toEqual([0, 50, 100]);
    expect(niceTicks(1234)).toEqual([0, 500, 1000, 1500]);
    expect(niceTicks(1)).toEqual([0, 1]);
    expect(niceTicks(2)).toEqual([0, 1, 2]);
    expect(niceTicks(3)).toEqual([0, 1, 2, 3]);
    expect(niceTicks(48_000)[niceTicks(48_000).length - 1]).toBeGreaterThanOrEqual(48_000);
  });

  it('always covers the maximum and uses few ticks', () => {
    for (const max of [1, 2, 3, 9, 10, 11, 99, 101, 999, 12_345, 987_654]) {
      const t = niceTicks(max);
      expect(t[0]).toBe(0);
      expect(t[t.length - 1]).toBeGreaterThanOrEqual(max);
      expect(t.length).toBeLessThanOrEqual(7);
      expect(new Set(t).size).toBe(t.length); // no duplicate ticks
    }
  });

  it('handles an empty chart', () => {
    expect(niceTicks(0)).toEqual([0, 1]);
    expect(niceTicks(-5)).toEqual([0, 1]);
    expect(niceTicks(NaN)).toEqual([0, 1]);
  });
});

describe('xTickIndexes', () => {
  it('labels everything when there are few buckets, and includes both ends otherwise', () => {
    expect(xTickIndexes(0)).toEqual([]);
    expect(xTickIndexes(3)).toEqual([0, 1, 2]);
    const t = xTickIndexes(120);
    expect(t).toHaveLength(6);
    expect(t[0]).toBe(0);
    expect(t[t.length - 1]).toBe(119);
    expect([...t].sort((a, c) => a - c)).toEqual(t);
  });
});

describe('layoutBars', () => {
  it('stacks dropped on the baseline, then forwarded, then other, with a gap between segments', () => {
    const [bar] = layoutBars([b(60, 30, 10)], 100, 100, 100);
    expect(bar.segments.map((s) => s.key)).toEqual([...STACK]);
    const [drop, fwd, other] = bar.segments;
    expect(drop.y + drop.h).toBeCloseTo(100); // sits on the baseline
    expect(drop.h).toBeCloseTo(30);
    expect(fwd.y + fwd.h).toBeCloseTo(drop.y - GAP);
    expect(other.y + other.h).toBeCloseTo(fwd.y - GAP);
    expect(bar.total).toBe(100);
  });

  it('draws only the series that have flows, and never an empty bar', () => {
    const [bar] = layoutBars([b(5, 0, 0)], 100, 100, 10);
    expect(bar.segments.map((s) => s.key)).toEqual(['forwarded']);
    expect(layoutBars([b(0, 0, 0)], 100, 100, 10)[0].segments).toEqual([]);
  });

  it('keeps a lone drop visible next to a huge forwarded count', () => {
    const [bar] = layoutBars([b(1_000_000, 1)], 100, 100, 1_000_000);
    const drop = bar.segments.find((s) => s.key === 'dropped')!;
    expect(drop.h).toBe(MIN_SEGMENT);
    expect(drop.value).toBe(1); // the true value is still what the tooltip reports
  });

  it('caps bar width at 24px and centers it in a wider slot, whose full width is the hover target', () => {
    const [bar] = layoutBars([b(1, 0)], 400, 100, 1);
    expect(bar.barW).toBe(MAX_BAR_WIDTH);
    expect(bar.slotW).toBe(400);
    expect(bar.barX).toBeCloseTo((400 - MAX_BAR_WIDTH) / 2);
  });

  it('shrinks bars in a crowded chart but leaves a gap between neighbours', () => {
    const bars = layoutBars(Array.from({ length: 120 }, () => b(1, 0)), 900, 100, 1);
    expect(bars[0].barW).toBeCloseTo(900 / 120 - GAP);
    expect(bars[1].barX - (bars[0].barX + bars[0].barW)).toBeCloseTo(GAP);
    expect(Math.max(...bars.map((x) => x.slotX + x.slotW))).toBeCloseTo(900);
  });

  it('returns nothing for no data or a zero axis', () => {
    expect(layoutBars([], 100, 100, 10)).toEqual([]);
    expect(layoutBars([b(1, 1)], 100, 100, 0)).toEqual([]);
  });

  it('heights are proportional to the axis maximum', () => {
    const [bar] = layoutBars([b(50, 0)], 100, 200, 100);
    expect(bar.segments[0].h).toBeCloseTo(100);
  });
});

describe('series identity', () => {
  it('assigns each series a fixed slot regardless of which are present', () => {
    expect(SERIES.map((s) => [s.key, s.cssVar])).toEqual([
      ['forwarded', '--series-1'],
      ['dropped', '--series-2'],
      ['other', '--series-3'],
    ]);
  });
});

describe('peakIndex and totals', () => {
  it('finds the busiest bucket, or none', () => {
    expect(peakIndex([b(1, 0), b(3, 4), b(2, 0)])).toBe(1);
    expect(peakIndex([b(0, 0), b(0, 0)])).toBe(-1);
    expect(peakIndex([])).toBe(-1);
    expect(bucketTotal(b(1, 2, 3))).toBe(6);
  });
});

describe('rangeFor', () => {
  const now = new Date('2026-09-24T12:00:00Z');
  const none = { from: '', to: '' };

  it('computes preset ranges ending now', () => {
    expect(rangeFor('1h', now, none)).toEqual({ from: '2026-09-24T11:00:00.000Z', to: '2026-09-24T12:00:00.000Z' });
    expect(rangeFor('7d', now, none)).toEqual({ from: '2026-09-17T12:00:00.000Z', to: '2026-09-24T12:00:00.000Z' });
    expect(rangeFor('15m', now, none)).toEqual({ from: '2026-09-24T11:45:00.000Z', to: '2026-09-24T12:00:00.000Z' });
  });

  it('converts a custom local range to ISO and validates it', () => {
    const r = rangeFor('custom', now, { from: '2026-09-24T09:00', to: '2026-09-24T10:30' });
    expect(r).toEqual({ from: new Date('2026-09-24T09:00').toISOString(), to: new Date('2026-09-24T10:30').toISOString() });
    expect(rangeFor('custom', now, { from: '', to: '2026-09-24T10:30' })).toEqual({ error: 'Choose both a start and an end time.' });
    expect(rangeFor('custom', now, { from: 'nope', to: 'nope' })).toHaveProperty('error');
    expect(rangeFor('custom', now, { from: '2026-09-24T11:00', to: '2026-09-24T10:00' })).toEqual({ error: 'The start must be earlier than the end.' });
    expect(rangeFor('custom', now, { from: '2026-09-24T10:00', to: '2026-09-24T10:00' })).toHaveProperty('error');
    expect(rangeFor('custom', now, { from: '2026-07-01T00:00', to: '2026-09-24T00:00' })).toEqual({ error: 'A range can cover at most 30 days.' });
  });
});

describe('labels (local time)', () => {
  it('shows just the time within a day, and the date once a range spans days', () => {
    const t = local(2026, 9, 24, 3, 5);
    expect(tickLabel(t, 3600)).toBe('03:05');
    expect(tickLabel(t, 24 * 3600)).toBe('03:05');
    expect(tickLabel(t, 7 * 86_400)).toBe('Sep 24 03:05');
  });

  it('describes a bucket by its start and end, adding the date only when it crosses midnight', () => {
    expect(bucketRangeLabel(local(2026, 9, 24, 3, 5), 60)).toBe('Sep 24 03:05 – 03:06');
    expect(bucketRangeLabel(local(2026, 9, 24, 23, 0), 3600)).toBe('Sep 24 23:00 – Sep 25 00:00');
  });

  it('formats counts with thousands separators', () => {
    expect(formatCount(1234567)).toBe('1,234,567');
    expect(formatCount(0)).toBe('0');
  });
});
