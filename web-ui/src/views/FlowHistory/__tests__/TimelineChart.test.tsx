// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { render, screen, fireEvent, within } from '@testing-library/react';
import { describe, it, expect } from 'vitest';
import TimelineChart from '../TimelineChart';
import type { TimelineBucketData } from '../../../services/api';

const local = (h: number, mi: number) => new Date(2026, 8, 24, h, mi).toISOString();
const bucket = (mi: number, forwarded: number, dropped: number, other = 0): TimelineBucketData => ({ start: local(3, mi), forwarded, dropped, other });

const DATA = [bucket(0, 10, 0), bucket(1, 40, 5), bucket(2, 0, 0), bucket(3, 25, 2, 1)];
const renderChart = (buckets = DATA, loading = false) =>
  render(<TimelineChart buckets={buckets} bucketSecs={60} rangeSecs={3600} loading={loading} />);
const hits = (c: HTMLElement) => Array.from(c.querySelectorAll('rect.viz-hit'));

describe('TimelineChart', () => {
  it('renders nothing without buckets', () => {
    const { container } = render(<TimelineChart buckets={[]} bucketSecs={60} rangeSecs={3600} loading={false} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('has a legend of the series present, and shows "Other verdicts" only when there are any', () => {
    const { rerender } = renderChart();
    const legend = screen.getByRole('list', { name: 'Legend' });
    expect(within(legend).getAllByRole('listitem').map((l) => l.textContent)).toEqual(['Forwarded', 'Dropped', 'Other verdicts']);
    rerender(<TimelineChart buckets={[bucket(0, 3, 1)]} bucketSecs={60} rangeSecs={3600} loading={false} />);
    expect(within(screen.getByRole('list', { name: 'Legend' })).getAllByRole('listitem').map((l) => l.textContent)).toEqual(['Forwarded', 'Dropped']);
  });

  it('colors each series with its fixed slot, so a series never changes color', () => {
    const { container } = renderChart();
    const fills = Array.from(container.querySelectorAll('svg path, svg rect:not(.viz-hit)')).map((e) => e.getAttribute('fill'));
    expect(new Set(fills.filter(Boolean))).toEqual(new Set(['var(--series-1)', 'var(--series-2)', 'var(--series-3)']));
    const swatches = Array.from(container.querySelectorAll('.viz-swatch')).map((s) => (s as unknown as HTMLElement).style.background);
    expect(swatches).toEqual(['var(--series-1)', 'var(--series-2)', 'var(--series-3)']);
  });

  it('draws no mark for an empty bucket or an absent series', () => {
    const { container } = renderChart([bucket(0, 0, 0), bucket(1, 5, 0)]);
    expect(container.querySelectorAll('svg path')).toHaveLength(1); // one bar, one segment
    expect(container.querySelectorAll('svg rect:not(.viz-hit)')).toHaveLength(0);
  });

  it('rounds only the data end of a column: the top segment is a path, lower ones are square rects', () => {
    const { container } = renderChart([bucket(1, 40, 5)]);
    expect(container.querySelectorAll('svg path')).toHaveLength(1);
    expect(container.querySelectorAll('svg rect:not(.viz-hit)')).toHaveLength(1);
    expect(container.querySelector('svg path')!.getAttribute('fill')).toBe('var(--series-1)'); // forwarded is on top
  });

  it('labels only the peak, once', () => {
    const { container } = renderChart();
    const labels = Array.from(container.querySelectorAll('.viz-peak'));
    expect(labels.map((l) => l.textContent)).toEqual(['45']);
  });

  it('has a focusable hover target per bucket with a full description', () => {
    const { container } = renderChart();
    const targets = hits(container);
    expect(targets).toHaveLength(4);
    expect(targets.every((t) => t.getAttribute('tabindex') === '0')).toBe(true);
    expect(targets[1].getAttribute('aria-label')).toMatch(/40 forwarded, 5 dropped$/);
    expect(targets[3].getAttribute('aria-label')).toMatch(/25 forwarded, 2 dropped, 1 other$/);
  });

  it('shows a tooltip with every series and the total on hover, and hides it on leave', () => {
    const { container } = renderChart();
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
    fireEvent.pointerEnter(hits(container)[1]);
    const tip = screen.getByRole('status');
    expect(within(tip).getByText('Forwarded').nextSibling).toHaveTextContent('40');
    expect(within(tip).getByText('Dropped').nextSibling).toHaveTextContent('5');
    expect(within(tip).getByText('Total').nextSibling).toHaveTextContent('45');
    expect(within(tip).getByText(/Sep 24 03:01 – 03:02/)).toBeInTheDocument();
    fireEvent.pointerLeave(hits(container)[1]);
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('shows the same tooltip on keyboard focus as on hover', () => {
    const { container } = renderChart();
    fireEvent.focus(hits(container)[3]);
    const tip = screen.getByRole('status');
    expect(within(tip).getByText('Other verdicts').nextSibling).toHaveTextContent('1');
    expect(within(tip).getByText('Total').nextSibling).toHaveTextContent('28');
    fireEvent.blur(hits(container)[3]);
    expect(screen.queryByRole('status')).not.toBeInTheDocument();
  });

  it('flips the tooltip to the left of a bucket in the right-hand half', () => {
    const many = Array.from({ length: 10 }, (_, i) => bucket(i, 1, 0));
    const { container } = renderChart(many);
    fireEvent.pointerEnter(hits(container)[1]);
    expect(screen.getByRole('status').style.transform).toContain('translateX(14px)');
    fireEvent.pointerLeave(hits(container)[1]);
    fireEvent.pointerEnter(hits(container)[9]);
    expect(screen.getByRole('status').style.transform).toContain('calc(-100%');
  });

  it('offers every value in a table, including empty intervals, without needing a tooltip', () => {
    renderChart();
    const table = screen.getByRole('table');
    const rows = within(table).getAllByRole('row');
    expect(rows).toHaveLength(5); // header + 4 buckets, the empty one included
    expect(within(rows[0]).getAllByRole('columnheader').map((h) => h.textContent)).toEqual(['Interval', 'Forwarded', 'Dropped', 'Other', 'Total']);
    expect(within(rows[2]).getAllByRole('cell').map((c) => c.textContent)).toEqual([expect.stringContaining('03:01'), '40', '5', '0', '45']);
    expect(within(rows[3]).getAllByRole('cell').slice(1).map((c) => c.textContent)).toEqual(['0', '0', '0', '0']);
  });

  it('leaves the Other column out of the table when no bucket has any', () => {
    renderChart([bucket(0, 3, 1)]);
    expect(within(screen.getByRole('table')).queryByRole('columnheader', { name: 'Other' })).not.toBeInTheDocument();
  });

  it('formats large counts with separators', () => {
    const { container } = renderChart([bucket(0, 1_234_567, 0), bucket(1, 5, 0)]);
    expect(container.querySelector('.viz-peak')!.textContent).toBe('1,234,567');
    expect(hits(container)[0].getAttribute('aria-label')).toContain('1,234,567 forwarded');
  });

  it('holds the previous render, dimmed, while a refetch is in flight', () => {
    const { container, rerender } = renderChart(DATA, false);
    expect(container.querySelector('.viz-root')!.getAttribute('data-loading')).toBe('false');
    rerender(<TimelineChart buckets={DATA} bucketSecs={60} rangeSecs={3600} loading />);
    expect(container.querySelector('.viz-root')!.getAttribute('data-loading')).toBe('true');
    expect(container.querySelectorAll('rect.viz-hit')).toHaveLength(4); // still drawn: no skeleton, no layout jump
  });

  it('summarises the total for assistive technology', () => {
    const { container } = renderChart();
    expect(container.querySelector('svg')!.getAttribute('aria-label')).toBe('Flows per 1-minute interval, 83 in total');
  });
});
