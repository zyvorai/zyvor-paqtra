// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { useThemeStore } from '../stores/themeStore';
import { useAuthStore } from '../stores/authStore';
import { usePreferencesStore } from '../stores/preferencesStore';

describe('Theme Store', () => {
  let initialIsDark: boolean;

  beforeEach(() => {
    // Capture and restore the initial state before each test
    initialIsDark = useThemeStore.getState().isDark;
  });

  afterEach(() => {
    // Reset to the captured initial state after each test
    if (useThemeStore.getState().isDark !== initialIsDark) {
      useThemeStore.getState().toggle();
    }
  });

  it('has initial dark mode state', () => {
    expect(typeof useThemeStore.getState().isDark).toBe('boolean');
  });

  it('toggle switches theme', () => {
    const initial = useThemeStore.getState().isDark;
    useThemeStore.getState().toggle();
    expect(useThemeStore.getState().isDark).toBe(!initial);
  });

  it('double toggle restores original state', () => {
    const initial = useThemeStore.getState().isDark;
    useThemeStore.getState().toggle();
    useThemeStore.getState().toggle();
    expect(useThemeStore.getState().isDark).toBe(initial);
  });
});

describe('Auth Store', () => {
  it('has initial state', () => {
    const state = useAuthStore.getState();
    expect(state.authRequired).toBe(true);
    expect(state.sessionReady).toBe(false);
    expect(typeof state.login).toBe('function');
    expect(typeof state.logout).toBe('function');
    expect(typeof state.checkSession).toBe('function');
  });

  it('logout clears token and requires auth', () => {
    useAuthStore.getState().logout();
    expect(useAuthStore.getState().token).toBeNull();
    expect(useAuthStore.getState().isAuthenticated).toBe(false);
    expect(useAuthStore.getState().authRequired).toBe(true);
  });
});

describe('Preferences Store', () => {
  beforeEach(() => {
    usePreferencesStore.getState().resetPrefs();
  });

  it('has default values', () => {
    const state = usePreferencesStore.getState();
    expect(state.tablePageSize).toBe(25);
    expect(state.autoRefresh).toBe(false);
    expect(state.refreshInterval).toBe(10);
    expect(state.compactMode).toBe(false);
    expect(state.showTimestamps).toBe(true);
    expect(state.notifications).toBe(false);
    expect(state.language).toBe('en');
  });

  it('setPref updates a preference', () => {
    usePreferencesStore.getState().setPref('tablePageSize', 50);
    expect(usePreferencesStore.getState().tablePageSize).toBe(50);
  });

  it('setPref updates language', () => {
    usePreferencesStore.getState().setPref('language', 'es');
    expect(usePreferencesStore.getState().language).toBe('es');
  });

  it('resetPrefs restores defaults', () => {
    usePreferencesStore.getState().setPref('compactMode', true);
    usePreferencesStore.getState().setPref('tablePageSize', 100);
    usePreferencesStore.getState().resetPrefs();
    expect(usePreferencesStore.getState().compactMode).toBe(false);
    expect(usePreferencesStore.getState().tablePageSize).toBe(25);
  });
});
