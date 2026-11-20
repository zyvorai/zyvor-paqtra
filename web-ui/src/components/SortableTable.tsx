// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useMemo } from 'react';
import { ArrowUp, ArrowDown } from 'lucide-react';

interface Column<T> {
  key: keyof T & string;
  label: string;
  align?: 'left' | 'right' | 'center';
  sortable?: boolean;
  render?: (value: T[keyof T], row: T) => React.ReactNode;
}

interface SortableTableProps<T> {
  columns: Column<T>[];
  data: T[];
  loading?: boolean;
  emptyMessage?: string;
  onRowClick?: (row: T) => void;
  sectionTitle?: string;
  sectionIcon?: React.ReactNode;
  count?: number;
  rowKey?: keyof T & string;
}

function SortableTable<T extends Record<string, unknown>>({
  columns,
  data,
  loading,
  emptyMessage,
  onRowClick,
  sectionTitle,
  sectionIcon,
  count,
  rowKey,
}: SortableTableProps<T>) {
  const [sortField, setSortField] = useState<string | null>(null);
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>('asc');

  const handleSort = (key: string) => {
    if (sortField === key) {
      setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'));
    } else {
      setSortField(key);
      setSortDir('asc');
    }
  };

  const sorted = useMemo(() => {
    if (!sortField) return data;
    return [...data].sort((a, b) => {
      const aVal = a[sortField];
      const bVal = b[sortField];
      if (aVal == null && bVal == null) return 0;
      if (aVal == null) return 1;
      if (bVal == null) return -1;
      const cmp = typeof aVal === 'number' && typeof bVal === 'number'
        ? aVal - bVal
        : String(aVal).localeCompare(String(bVal));
      return sortDir === 'asc' ? cmp : -cmp;
    });
  }, [data, sortField, sortDir]);

  return (
    <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
      {sectionTitle && (
        <div className="px-5 py-4 border-b border-slate-700/50 flex items-center justify-between">
          <div className="flex items-center gap-3">
            {sectionIcon}
            <h2 className="text-lg font-semibold text-white">{sectionTitle}</h2>
          </div>
          {count != null && (
            <span className="text-xs font-medium text-slate-400 bg-slate-700/50 px-2.5 py-1 rounded-full">
              {count} items
            </span>
          )}
        </div>
      )}
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-slate-700/50 bg-slate-900/50">
              {columns.map((col) => (
                <th
                  key={col.key}
                  className={`px-4 py-3 font-semibold text-slate-400 uppercase tracking-wider ${
                    col.align === 'right' ? 'text-right' : col.align === 'center' ? 'text-center' : 'text-left'
                  } ${col.sortable !== false ? 'cursor-pointer select-none hover:text-slate-200 transition-colors' : ''}`}
                  onClick={() => col.sortable !== false && handleSort(col.key)}
                >
                  <span className="inline-flex items-center gap-1">
                    {col.label}
                    {sortField === col.key && (
                      sortDir === 'asc'
                        ? <ArrowUp className="w-3 h-3" />
                        : <ArrowDown className="w-3 h-3" />
                    )}
                  </span>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sorted.length === 0 && !loading ? (
              <tr>
                <td colSpan={columns.length} className="px-4 py-12 text-center text-slate-400">
                  {emptyMessage ?? 'No data'}
                </td>
              </tr>
            ) : sorted.map((row, i) => (
              <tr
                key={rowKey && row[rowKey] != null ? String(row[rowKey]) : `row-${i}`}
                className={`border-b border-slate-700/30 table-row-hover ${onRowClick ? 'cursor-pointer' : ''}`}
                onClick={() => onRowClick?.(row)}
              >
                {columns.map((col) => (
                  <td
                    key={col.key}
                    className={`px-4 py-2.5 ${
                      col.align === 'right' ? 'text-right' : col.align === 'center' ? 'text-center' : 'text-left'
                    }`}
                  >
                    {col.render ? col.render(row[col.key], row) : String(row[col.key] ?? '')}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export default SortableTable;
