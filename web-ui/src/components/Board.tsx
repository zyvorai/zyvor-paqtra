// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import type { ReactNode } from 'react';

/** Netra-style page board primitives shared by every Paqtra view. */

export function Board({ children }: { children: ReactNode }) {
  return <div className="grid">{children}</div>;
}

export function Card({
  children,
  span,
  className = '',
}: {
  children: ReactNode;
  span?: 2 | 3;
  className?: string;
}) {
  const spanClass = span === 2 ? ' span2' : span === 3 ? ' span3' : '';
  return <section className={`card${spanClass} ${className}`.trim()}>{children}</section>;
}

export function Eyebrow({ children }: { children: ReactNode }) {
  return <p className="eyebrow">{children}</p>;
}

export function Metrics({ children }: { children: ReactNode }) {
  return <div className="metrics">{children}</div>;
}

export function Metric({ value, label }: { value: ReactNode; label: string }) {
  return (
    <div>
      <b>{value}</b>
      <span>{label}</span>
    </div>
  );
}

export function Warning({ children }: { children: ReactNode }) {
  return <p className="warning">{children}</p>;
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="empty-state">{children}</p>;
}

export function Toolbar({ children }: { children: ReactNode }) {
  return <div className="filters board-toolbar">{children}</div>;
}
