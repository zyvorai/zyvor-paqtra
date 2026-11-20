// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useCallback, useEffect } from 'react';
import { Bell, BellOff } from 'lucide-react';

const STORAGE_KEY = 'paqtra-notifications';

// eslint-disable-next-line react-refresh/only-export-components
export function useNotifications() {
  const [permission, setPermission] = useState<NotificationPermission>(
    typeof Notification !== 'undefined' ? Notification.permission : 'denied',
  );

  const requestPermission = useCallback(async () => {
    if (typeof Notification === 'undefined') return 'denied' as NotificationPermission;
    const result = await Notification.requestPermission();
    setPermission(result);
    return result;
  }, []);

  const notify = useCallback(
    (title: string, body: string, options?: NotificationOptions) => {
      if (typeof Notification === 'undefined') return;
      if (Notification.permission !== 'granted') return;

      const enabled = localStorage.getItem(STORAGE_KEY);
      if (enabled === 'false') return;

      try {
        new Notification(title, { body, ...options });
      } catch {
        // Silently ignore if notifications fail (e.g., in some environments)
      }
    },
    [],
  );

  return { permission, requestPermission, notify };
}

export const NotificationToggle: React.FC = () => {
  const [enabled, setEnabled] = useState(() => {
    return localStorage.getItem(STORAGE_KEY) !== 'false';
  });
  const { requestPermission } = useNotifications();

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, String(enabled));
  }, [enabled]);

  const handleToggle = async () => {
    if (!enabled) {
      // Turning on: request permission first
      const perm = await requestPermission();
      if (perm === 'granted') {
        setEnabled(true);
      }
    } else {
      setEnabled(false);
    }
  };

  return (
    <button
      onClick={handleToggle}
      className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-slate-700/50 text-sm text-slate-400 hover:text-white hover:bg-slate-700/50 transition-colors"
      title={enabled ? 'Disable notifications' : 'Enable notifications'}
    >
      {enabled ? <Bell className="w-4 h-4" /> : <BellOff className="w-4 h-4" />}
      <span>{enabled ? 'Notifications On' : 'Notifications Off'}</span>
    </button>
  );
};
