// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { create } from 'zustand';

interface ThemeState {
  isDark: boolean;
  toggle: () => void;
}

export const useThemeStore = create<ThemeState>((set) => ({
  isDark: localStorage.getItem('paqtra-theme') !== 'light',
  toggle: () =>
    set((state) => {
      const next = !state.isDark;
      localStorage.setItem('paqtra-theme', next ? 'dark' : 'light');
      document.documentElement.classList.toggle('dark', next);
      document.documentElement.classList.toggle('light-theme', !next);
      return { isDark: next };
    }),
}));
