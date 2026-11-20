// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react';
import { flowDuration, workloadLabel } from './datapath';

/** `targets` lists the indexes this node feeds in the next column (default: every active node there). */
export type DatapathNode = { label: string; sub?: string; active: boolean; targets?: number[] };
export type DatapathColumn = { title: string; nodes: DatapathNode[] };

type DatapathHeroProps = {
  columns: DatapathColumn[];
  /** Flow rate driving dot speed; busier datapath, faster dots. */
  flowsPerSecond?: number;
  /** Red dots are drawn only while drops are actually increasing. */
  dropsPerSecond?: number;
  label: string;
};

type Placed = DatapathNode & { id: string; x: number; y: number; w: number; h: number };

function useMedia(query: string): boolean {
  const get = () => typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia(query).matches;
  const [match, setMatch] = useState(get);
  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return;
    const m = window.matchMedia(query);
    const on = () => setMatch(m.matches);
    m.addEventListener('change', on);
    return () => m.removeEventListener('change', on);
  }, [query]);
  return match;
}

function link(a: Placed, b: Placed, vertical: boolean): string {
  if (vertical) {
    const ay = a.y + a.h / 2;
    const by = b.y - b.h / 2;
    const my = (ay + by) / 2;
    return `M${a.x},${ay} C${a.x},${my} ${b.x},${my} ${b.x},${by}`;
  }
  const ax = a.x + a.w / 2;
  const bx = b.x - b.w / 2;
  const mx = (ax + bx) / 2;
  return `M${ax},${a.y} C${mx},${a.y} ${mx},${b.y} ${bx},${b.y}`;
}

export default function DatapathHero({ columns, flowsPerSecond, dropsPerSecond, label }: DatapathHeroProps) {
  const vertical = useMedia('(max-width: 640px)');
  const reduced = useMedia('(prefers-reduced-motion: reduce)');

  const W = vertical ? 360 : 960;
  const H = vertical ? 520 : 300;
  const nw = vertical ? 76 : 168;
  const nh = vertical ? 36 : 50;
  const n = columns.length;
  const main = columns.map((_, i) => (vertical ? 50 + (i * 420) / Math.max(1, n - 1) : 110 + (i * 760) / Math.max(1, n - 1)));

  const placed: Placed[][] = columns.map((col, ci) =>
    col.nodes.map((node, i) => {
      const cross = ((i + 1) / (col.nodes.length + 1)) * (vertical ? W : H);
      return {
        ...node,
        label: workloadLabel(node.label, vertical ? 10 : 18),
        sub: vertical ? undefined : node.sub,
        id: `${ci}-${i}`,
        ...(vertical ? { x: cross, y: main[ci] } : { x: main[ci], y: cross }),
        w: nw,
        h: nh,
      };
    }),
  );

  const links: { d: string; live: boolean; key: string }[] = [];
  for (let ci = 0; ci + 1 < placed.length; ci++) {
    const next = placed[ci + 1];
    const targets = next.filter((x) => x.active).length ? next.filter((x) => x.active) : next.slice(0, 1);
    for (const a of placed[ci]) {
      const to = a.targets ? a.targets.map((i) => next[i]).filter(Boolean) : targets;
      for (const b of to) links.push({ key: a.id + b.id, d: link(a, b, vertical), live: a.active && b.active });
    }
  }

  const dur = flowDuration(flowsPerSecond);
  const dropping = (dropsPerSecond ?? 0) > 0;
  const first = placed[0] || [];
  const firstTarget = placed[1]?.find((x) => x.active) || placed[1]?.[0];

  return (
    <figure className={vertical ? 'dp-hero dp-hero--vertical' : 'dp-hero'} aria-label={label}>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-hidden="true" preserveAspectRatio="xMidYMid meet">
        {!vertical &&
          columns.map((c, i) => (
            <text key={'t' + i} className="dp-col-title" x={main[i]} y={18} textAnchor="middle">
              {c.title}
            </text>
          ))}
        {links.map((l) => (
          <path key={l.key} d={l.d} className={l.live ? 'dp-link dp-link--live' : 'dp-link'} />
        ))}
        {!reduced &&
          links
            .filter((l) => l.live)
            .map((l, i) =>
              [0, 1].map((k) => (
                <circle key={l.key + k} r={vertical ? 3 : 3.5} className="dp-dot">
                  <animateMotion dur={`${dur}s`} begin={`-${((i * 0.37 + (k * dur) / 2) % dur).toFixed(2)}s`} repeatCount="indefinite" path={l.d} />
                </circle>
              )),
            )}
        {!reduced &&
          dropping &&
          firstTarget &&
          first.slice(0, 2).map((a, i) => (
            <circle key={'b' + i} r={vertical ? 3.5 : 4} className="dp-dot dp-dot--blocked">
              <animateMotion dur="2.4s" begin={`-${i * 1.2}s`} repeatCount="indefinite" path={link(a, firstTarget, vertical)} />
              <animate attributeName="opacity" values="1;1;0" keyTimes="0;0.85;1" dur="2.4s" begin={`-${i * 1.2}s`} repeatCount="indefinite" />
            </circle>
          ))}
        {placed.flat().map((p) => (
          <g key={p.id} className={p.active ? 'dp-node dp-node--active' : 'dp-node'}>
            <rect x={p.x - p.w / 2} y={p.y - p.h / 2} width={p.w} height={p.h} rx={p.h / 2} />
            <text x={p.x} y={p.sub ? p.y - 3 : p.y + 4} textAnchor="middle" className="dp-node-label">
              {p.label}
            </text>
            {p.sub && (
              <text x={p.x} y={p.y + 13} textAnchor="middle" className="dp-node-sub">
                {p.sub}
              </text>
            )}
          </g>
        ))}
      </svg>
    </figure>
  );
}
