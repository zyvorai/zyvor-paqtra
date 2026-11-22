// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useState } from 'react';
import { Outlet, useLocation, useNavigate } from 'react-router-dom';
import Nav from './Nav';
import PageHero from './PageHero';
import { useAuthStore } from '../stores/authStore';
import { heroForPath } from '../navConfig';
import { applyTheme, readStoredTheme, toggleTheme, type Theme } from '../theme';

export default function MainLayout() {
  const location = useLocation();
  const navigate = useNavigate();
  const logout = useAuthStore((s) => s.logout);
  const [theme, setTheme] = useState<Theme>(() => {
    const t = readStoredTheme();
    applyTheme(t);
    return t;
  });

  const isOverview = location.pathname === '/';
  const hero = isOverview ? null : heroForPath(location.pathname);

  return (
    <>
      <Nav
        theme={theme}
        onToggleTheme={() => setTheme((t) => toggleTheme(t))}
        onLogout={() => {
          logout();
          navigate('/');
        }}
      />
      <main>
        <div key={location.pathname}>
          {!isOverview && hero && <PageHero eyebrow={hero.eyebrow} title={hero.title} lede={hero.lede} tint={hero.tint} />}
          <div className={isOverview ? undefined : 'view-board'}>
            <Outlet />
          </div>
        </div>
      </main>
    </>
  );
}
