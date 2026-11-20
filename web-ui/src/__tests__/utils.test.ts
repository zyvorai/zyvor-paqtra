// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { describe, it, expect } from 'vitest';
import { safeFixed, safeLocale, safePct, safeArray } from '../utils/safe';
import { formatBytes, formatDuration, formatRelativeTime, formatNumber, getStatusColor } from '../utils/formatters';

describe('safe utils', () => {
  describe('safeFixed', () => {
    it('handles normal numbers', () => { expect(safeFixed(3.14159, 2)).toBe('3.14'); });
    it('handles undefined', () => { expect(safeFixed(undefined)).toBe('0.0'); });
    it('handles null', () => { expect(safeFixed(null, 2)).toBe('0.00'); });
    it('handles NaN', () => { expect(safeFixed(NaN)).toBe('0.0'); });
    it('handles zero', () => { expect(safeFixed(0)).toBe('0.0'); });
  });

  describe('safeLocale', () => {
    it('handles normal numbers', () => { expect(safeLocale(1234)).toBeTruthy(); });
    it('handles undefined', () => { expect(safeLocale(undefined)).toBe('0'); });
    it('handles null', () => { expect(safeLocale(null)).toBe('0'); });
  });

  describe('safePct', () => {
    it('calculates percentage', () => { expect(safePct(50, 100)).toBe(50); });
    it('guards division by zero', () => { expect(safePct(50, 0)).toBe(0); });
    it('guards null total', () => { expect(safePct(50, null)).toBe(0); });
    it('guards null used', () => { expect(safePct(null, 100)).toBe(0); });
    it('caps at 100', () => { expect(safePct(200, 100)).toBe(100); });
  });

  describe('safeArray', () => {
    it('returns array as-is', () => { expect(safeArray([1, 2])).toEqual([1, 2]); });
    it('returns empty for undefined', () => { expect(safeArray(undefined)).toEqual([]); });
    it('returns empty for null', () => { expect(safeArray(null)).toEqual([]); });
  });
});

describe('formatters', () => {
  describe('formatBytes', () => {
    it('formats bytes', () => { expect(formatBytes(500)).toBe('500 B'); });
    it('formats KB', () => { expect(formatBytes(1500)).toBe('1.5 KB'); });
    it('formats MB', () => { expect(formatBytes(1500000)).toBe('1.5 MB'); });
    it('formats GB', () => { expect(formatBytes(1500000000)).toBe('1.5 GB'); });
    it('formats TB', () => { expect(formatBytes(1500000000000)).toBe('1.5 TB'); });
    it('handles zero', () => { expect(formatBytes(0)).toBe('0 B'); });
    it('handles null', () => { expect(formatBytes(null)).toBe('0 B'); });
  });

  describe('formatDuration', () => {
    it('formats seconds', () => { expect(formatDuration(45)).toBe('45s'); });
    it('formats minutes', () => { expect(formatDuration(125)).toBe('2m 5s'); });
    it('formats hours', () => { expect(formatDuration(3700)).toBe('1h 1m'); });
    it('formats days', () => { expect(formatDuration(90000)).toBe('1d 1h'); });
    it('handles zero', () => { expect(formatDuration(0)).toBe('0s'); });
    it('handles null', () => { expect(formatDuration(null)).toBe('0s'); });
  });

  describe('formatRelativeTime', () => {
    it('returns - for null', () => { expect(formatRelativeTime(null)).toBe('-'); });
    it('returns relative time', () => {
      const recent = new Date(Date.now() - 30000).toISOString();
      expect(formatRelativeTime(recent)).toMatch(/\d+s ago/);
    });
  });

  describe('formatNumber', () => {
    it('formats small numbers', () => { expect(formatNumber(42)).toBe('42'); });
    it('formats thousands', () => { expect(formatNumber(1500)).toBe('1.5K'); });
    it('formats millions', () => { expect(formatNumber(2500000)).toBe('2.5M'); });
    it('handles null', () => { expect(formatNumber(null)).toBe('0'); });
  });

  describe('getStatusColor', () => {
    it('returns green for healthy', () => { expect(getStatusColor('healthy')).toContain('green'); });
    it('returns red for error', () => { expect(getStatusColor('error')).toContain('red'); });
    it('returns yellow for degraded', () => { expect(getStatusColor('degraded')).toContain('yellow'); });
    it('returns muted for unknown', () => { expect(getStatusColor('unknown')).toContain('muted'); });
  });
});
