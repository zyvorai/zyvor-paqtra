// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { create } from 'zustand';

const STORAGE_KEY = 'paqtra-prefs';

interface Preferences {
  tablePageSize: number;
  autoRefresh: boolean;
  refreshInterval: number;
  compactMode: boolean;
  showTimestamps: boolean;
  notifications: boolean;
  language: string;
}

type PreferenceKey = keyof Preferences;

interface PreferencesState extends Preferences {
  setPref: <K extends PreferenceKey>(key: K, value: Preferences[K]) => void;
  resetPrefs: () => void;
}

const DEFAULTS: Preferences = {
  tablePageSize: 25,
  autoRefresh: false,
  refreshInterval: 10,
  compactMode: false,
  showTimestamps: true,
  notifications: false,
  language: 'en',
};

function loadPrefs(): Preferences {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<Preferences>;
      return { ...DEFAULTS, ...parsed };
    }
  } catch {
    // Ignore corrupt data
  }
  return { ...DEFAULTS };
}

function savePrefs(prefs: Preferences) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
}

export const usePreferencesStore = create<PreferencesState>((set, get) => ({
  ...loadPrefs(),

  setPref: (key, value) => {
    set({ [key]: value });
    const state = get();
    const prefs: Preferences = {
      tablePageSize: state.tablePageSize,
      autoRefresh: state.autoRefresh,
      refreshInterval: state.refreshInterval,
      compactMode: state.compactMode,
      showTimestamps: state.showTimestamps,
      notifications: state.notifications,
      language: state.language,
    };
    prefs[key] = value;
    savePrefs(prefs);
  },

  resetPrefs: () => {
    set({ ...DEFAULTS });
    savePrefs({ ...DEFAULTS });
  },
}));
