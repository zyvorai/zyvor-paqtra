// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useEffect, useRef, useState } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import type { Theme } from '../theme';
import { NAV_GROUPS } from '../navConfig';
import DigestChip from './DigestChip';

const OPEN_DELAY_MS = 120;
const CLOSE_DELAY_MS = 450;

export default function Nav({
  theme,
  onToggleTheme,
  onLogout,
}: {
  theme: Theme;
  onToggleTheme: () => void;
  onLogout: () => void;
}) {
  const navigate = useNavigate();
  const location = useLocation();
  const [openGroup, setOpenGroup] = useState<string | null>(null);
  const openTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const navRef = useRef<HTMLElement | null>(null);
  const triggerRefs = useRef<Record<string, HTMLButtonElement | null>>({});

  const clearTimers = () => {
    if (openTimer.current) clearTimeout(openTimer.current);
    if (closeTimer.current) clearTimeout(closeTimer.current);
    openTimer.current = null;
    closeTimer.current = null;
  };

  const scheduleOpen = (label: string) => {
    clearTimers();
    openTimer.current = setTimeout(() => setOpenGroup(label), OPEN_DELAY_MS);
  };

  const scheduleClose = () => {
    clearTimers();
    closeTimer.current = setTimeout(() => setOpenGroup(null), CLOSE_DELAY_MS);
  };

  const toggleGroup = (label: string) => {
    clearTimers();
    setOpenGroup((cur) => (cur === label ? null : label));
  };

  const go = (path: string) => {
    navigate(path);
    setOpenGroup(null);
  };

  useEffect(() => () => clearTimers(), []);

  useEffect(() => {
    setOpenGroup(null);
  }, [location.pathname]);

  useEffect(() => {
    if (!openGroup) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      const label = openGroup;
      setOpenGroup(null);
      triggerRefs.current[label]?.focus();
    };
    const onPointerDown = (e: MouseEvent) => {
      if (navRef.current && !navRef.current.contains(e.target as Node)) setOpenGroup(null);
    };
    document.addEventListener('keydown', onKeyDown);
    document.addEventListener('mousedown', onPointerDown);
    return () => {
      document.removeEventListener('keydown', onKeyDown);
      document.removeEventListener('mousedown', onPointerDown);
    };
  }, [openGroup]);

  const pathActive = (path: string) =>
    path === '/' ? location.pathname === '/' : location.pathname === path || location.pathname.startsWith(`${path}/`);

  return (
    <nav className="nav" aria-label="Global" ref={navRef}>
      <div className="nav-inner">
        <button type="button" className="brand" onClick={() => go('/')} aria-label="Paqtra home">
          <img src="/zyvor-logomark.svg" alt="" className="brand-mark" aria-hidden />
          Paqtra
        </button>
        <div className="navlinks">
          {NAV_GROUPS.map((g) =>
            g.children ? (
              <div
                key={g.label}
                className="navgroup"
                onMouseEnter={() => scheduleOpen(g.label)}
                onMouseLeave={scheduleClose}
              >
                <button
                  type="button"
                  ref={(el) => {
                    triggerRefs.current[g.label] = el;
                  }}
                  className={g.children.some((c) => pathActive(c.path)) ? 'active' : ''}
                  aria-haspopup="true"
                  aria-expanded={openGroup === g.label}
                  onClick={() => toggleGroup(g.label)}
                >
                  {g.label}
                </button>
                <div
                  className={`mega-panel${openGroup === g.label ? ' open' : ''}`}
                  role="region"
                  aria-label={g.label}
                  onMouseEnter={() => scheduleOpen(g.label)}
                  onMouseLeave={scheduleClose}
                >
                  <div className="mega-grid">
                    {g.children.map((c) => (
                      <button
                        key={`${g.label}-${c.path}-${c.label}`}
                        type="button"
                        className={pathActive(c.path) ? 'active' : ''}
                        aria-current={pathActive(c.path) ? 'page' : undefined}
                        onClick={() => go(c.path)}
                      >
                        <span className="mega-link-label">{c.label}</span>
                        <span className="mega-link-blurb">{c.blurb}</span>
                      </button>
                    ))}
                  </div>
                </div>
              </div>
            ) : (
              <button
                key={g.path}
                type="button"
                className={pathActive(g.path!) ? 'active' : ''}
                aria-current={pathActive(g.path!) ? 'page' : undefined}
                onClick={() => go(g.path!)}
              >
                {g.label}
              </button>
            ),
          )}
        </div>
        <div className="nav-actions">
          <DigestChip onOpen={() => go('/')} />
          <button type="button" className="theme-toggle" onClick={onLogout} aria-label="Log out" title="Log out">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" aria-hidden>
              <path d="M15 4H7a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h8" strokeLinecap="round" strokeLinejoin="round" />
              <path d="M10 12h11m0 0-3.5-3.5M21 12l-3.5 3.5" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </button>
          <button
            type="button"
            className="theme-toggle"
            onClick={onToggleTheme}
            aria-label={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
            title={theme === 'dark' ? 'Light' : 'Dark'}
          >
            {theme === 'dark' ? (
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" aria-hidden>
                <circle cx="12" cy="12" r="4" />
                <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M4.93 19.07l1.41-1.41M17.66 6.34l1.41-1.41" />
              </svg>
            ) : (
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" aria-hidden>
                <path d="M21 14.5A8.5 8.5 0 1 1 11.5 3a7 7 0 0 0 9.5 11.5z" />
              </svg>
            )}
          </button>
        </div>
      </div>
    </nav>
  );
}
