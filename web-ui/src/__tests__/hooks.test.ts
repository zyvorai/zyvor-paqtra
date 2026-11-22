// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { describe, it, expect, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useKeyboardShortcuts, shortcuts } from '../hooks/useKeyboardShortcuts';

describe('useKeyboardShortcuts', () => {
  it('exports shortcuts array', () => {
    expect(Array.isArray(shortcuts)).toBe(true);
    expect(shortcuts.length).toBeGreaterThan(0);
  });

  it('shortcuts have required fields', () => {
    shortcuts.forEach((s) => {
      expect(s).toHaveProperty('keys');
      expect(s).toHaveProperty('description');
      expect(s).toHaveProperty('category');
      expect(typeof s.keys).toBe('string');
      expect(typeof s.description).toBe('string');
      expect(['Navigation', 'Actions', 'General']).toContain(s.category);
    });
  });

  it('includes help shortcut', () => {
    const help = shortcuts.find((s) => s.keys === '?');
    expect(help).toBeDefined();
    expect(help?.description).toContain('keyboard shortcuts');
  });

  it('includes search shortcut', () => {
    const search = shortcuts.find((s) => s.keys === '/');
    expect(search).toBeDefined();
  });

  it('includes navigation shortcuts', () => {
    const nav = shortcuts.filter((s) => s.category === 'Navigation');
    expect(nav.length).toBeGreaterThan(5);
  });

  it('attaches keydown listener on mount and removes on unmount', () => {
    const addSpy = vi.spyOn(window, 'addEventListener');
    const removeSpy = vi.spyOn(window, 'removeEventListener');

    const { unmount } = renderHook(() =>
      useKeyboardShortcuts({})
    );

    expect(addSpy).toHaveBeenCalledWith('keydown', expect.any(Function));

    unmount();

    expect(removeSpy).toHaveBeenCalledWith('keydown', expect.any(Function));

    addSpy.mockRestore();
    removeSpy.mockRestore();
  });

  it('accepts handlers without throwing', () => {
    const handlers = {
      onToggleHelp: vi.fn(),
      onToggleSearch: vi.fn(),
      onRefresh: vi.fn(),
      navigate: vi.fn(),
    };

    const { unmount } = renderHook(() => useKeyboardShortcuts(handlers));
    // Hook should mount successfully with all handlers provided
    unmount();
  });

  it('calls onToggleHelp when ? is pressed', () => {
    const onToggleHelp = vi.fn();
    const { unmount } = renderHook(() =>
      useKeyboardShortcuts({ onToggleHelp })
    );

    window.dispatchEvent(new KeyboardEvent('keydown', { key: '?' }));
    expect(onToggleHelp).toHaveBeenCalledTimes(1);

    unmount();
  });

  it('calls onRefresh when r is pressed', () => {
    const onRefresh = vi.fn();
    const { unmount } = renderHook(() =>
      useKeyboardShortcuts({ onRefresh })
    );

    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'r' }));
    expect(onRefresh).toHaveBeenCalledTimes(1);

    unmount();
  });
});
