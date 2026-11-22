// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useState } from 'react';
import type { TimelineBucketData } from '../../services/api';
import {
  SERIES, bucketRangeLabel, bucketTotal, formatCount, layoutBars, niceTicks, peakIndex, tickLabel, xTickIndexes,
} from './timeline';
import './flowHistory.css';

// Logical drawing size; the SVG scales to its container.
const W = 960;
const PLOT_H = 220;
const M = { left: 48, right: 8, top: 18, bottom: 26 };
const PLOT_W = W - M.left - M.right;
const RADIUS = 4;

/** A column segment with a rounded top (the data end) and a square bottom. */
function roundedTop(x: number, y: number, w: number, h: number): string {
  const r = Math.max(0, Math.min(RADIUS, w / 2, h));
  return `M${x},${y + h} V${y + r} Q${x},${y} ${x + r},${y} H${x + w - r} Q${x + w},${y} ${x + w},${y + r} V${y + h} Z`;
}

interface Props {
  buckets: TimelineBucketData[];
  bucketSecs: number;
  /** Length of the range in seconds, to pick x-axis label detail. */
  rangeSecs: number;
  loading: boolean;
}

export default function TimelineChart({ buckets, bucketSecs, rangeSecs, loading }: Props) {
  const [active, setActive] = useState<number | null>(null);
  if (buckets.length === 0) return null;

  const maxTotal = Math.max(...buckets.map(bucketTotal));
  const ticks = niceTicks(maxTotal);
  const axisMax = ticks[ticks.length - 1];
  const bars = layoutBars(buckets, PLOT_W, PLOT_H, axisMax);
  const peak = peakIndex(buckets);
  const hasOther = buckets.some((b) => b.other > 0);
  const shown = SERIES.filter((s) => s.key !== 'other' || hasOther);
  const bucketLabel = bucketSecs >= 3600 ? `${bucketSecs / 3600}-hour` : `${bucketSecs / 60}-minute`;

  const at = active === null ? null : buckets[active];
  // Anchor the tooltip to the bucket; flip to its left in the right-hand half.
  const tipLeftPct = active === null ? 0 : ((M.left + bars[active].slotX + bars[active].slotW / 2) / W) * 100;
  const flip = tipLeftPct > 55;

  return (
    <div className="viz-root" data-loading={loading}>
      <ul className="viz-legend" aria-label="Legend">
        {shown.map((s) => (
          <li key={s.key}>
            <span className="viz-swatch" style={{ background: `var(${s.cssVar})` }} />
            {s.label}
          </li>
        ))}
      </ul>

      <svg
        className="viz-plot"
        viewBox={`0 0 ${W} ${PLOT_H + M.top + M.bottom}`}
        role="group"
        aria-label={`Flows per ${bucketLabel} interval, ${formatCount(buckets.reduce((n, b) => n + bucketTotal(b), 0))} in total`}
      >
        <g transform={`translate(${M.left},${M.top})`}>
          {ticks.map((t) => {
            const y = PLOT_H - (t / axisMax) * PLOT_H;
            return (
              <g key={t}>
                <line className={t === 0 ? 'viz-axis' : 'viz-grid'} x1={0} x2={PLOT_W} y1={y} y2={y} />
                <text className="viz-tick" x={-8} y={y + 4} textAnchor="end">{formatCount(t)}</text>
              </g>
            );
          })}

          {bars.map((bar) => (
            <g key={bar.index}>
              {bar.segments.map((seg, i) => {
                const fill = `var(${SERIES.find((s) => s.key === seg.key)!.cssVar})`;
                return i === bar.segments.length - 1 ? (
                  <path key={seg.key} d={roundedTop(bar.barX, seg.y, bar.barW, seg.h)} fill={fill} />
                ) : (
                  <rect key={seg.key} x={bar.barX} y={seg.y} width={bar.barW} height={seg.h} fill={fill} />
                );
              })}
            </g>
          ))}

          {/* Selective direct label: the peak only. The tooltip and table carry the rest. */}
          {peak >= 0 && buckets.length > 1 ? (
            <text
              className="viz-peak"
              x={bars[peak].barX + bars[peak].barW / 2}
              y={Math.max(-4, (bars[peak].segments[bars[peak].segments.length - 1]?.y ?? PLOT_H) - 6)}
              textAnchor="middle"
            >
              {formatCount(bars[peak].total)}
            </text>
          ) : null}

          {xTickIndexes(buckets.length).map((i) => (
            <text key={i} className="viz-tick" x={bars[i].barX + bars[i].barW / 2} y={PLOT_H + 18} textAnchor="middle">
              {tickLabel(buckets[i].start, rangeSecs)}
            </text>
          ))}

          {/* Hit targets: the whole slot, not just the painted bar. Same details on focus as on hover. */}
          {bars.map((bar) => {
            const b = buckets[bar.index];
            return (
              <rect
                key={`hit-${bar.index}`}
                className="viz-hit"
                data-active={active === bar.index}
                x={bar.slotX}
                y={0}
                width={bar.slotW}
                height={PLOT_H}
                tabIndex={0}
                role="img"
                aria-label={`${bucketRangeLabel(b.start, bucketSecs)}: ${formatCount(b.forwarded)} forwarded, ${formatCount(b.dropped)} dropped${b.other ? `, ${formatCount(b.other)} other` : ''}`}
                onPointerEnter={() => setActive(bar.index)}
                onPointerLeave={() => setActive(null)}
                onFocus={() => setActive(bar.index)}
                onBlur={() => setActive(null)}
              />
            );
          })}
        </g>
      </svg>

      {at ? (
        <div
          className="viz-tip"
          role="status"
          style={{ left: `${tipLeftPct}%`, top: 34, transform: flip ? 'translateX(calc(-100% - 14px))' : 'translateX(14px)' }}
        >
          <div className="viz-tip-title">{bucketRangeLabel(at.start, bucketSecs)}</div>
          {shown.map((s) => (
            <div className="viz-tip-row" key={s.key}>
              <span className="viz-tip-key" style={{ background: `var(${s.cssVar})` }} />
              <span>{s.label}</span>
              <span className="viz-tip-val">{formatCount(at[s.key])}</span>
            </div>
          ))}
          <div className="viz-tip-row viz-tip-total">
            <span>Total</span>
            <span className="viz-tip-val">{formatCount(bucketTotal(at))}</span>
          </div>
        </div>
      ) : null}

      <details>
        <summary>View as table</summary>
        <div className="viz-table-wrap">
          <table className="viz-table">
            <thead>
              <tr>
                <th>Interval</th>
                <th className="num">Forwarded</th>
                <th className="num">Dropped</th>
                {hasOther ? <th className="num">Other</th> : null}
                <th className="num">Total</th>
              </tr>
            </thead>
            <tbody>
              {buckets.map((b) => (
                <tr key={b.start}>
                  <td>{bucketRangeLabel(b.start, bucketSecs)}</td>
                  <td className="num">{formatCount(b.forwarded)}</td>
                  <td className="num">{formatCount(b.dropped)}</td>
                  {hasOther ? <td className="num">{formatCount(b.other)}</td> : null}
                  <td className="num strong">{formatCount(bucketTotal(b))}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </details>
    </div>
  );
}
