// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';

interface Column {
  key: string;
  label: string;
  align?: 'left' | 'right' | 'center';
  hideOnMobile?: boolean;
  render?: (value: unknown, row: Record<string, unknown>) => React.ReactNode;
}

interface ResponsiveTableProps {
  columns: Column[];
  data: Record<string, unknown>[];
  loading?: boolean;
  emptyMessage?: string;
  onRowClick?: (row: Record<string, unknown>) => void;
  rowKey?: string;
}

const ResponsiveTable: React.FC<ResponsiveTableProps> = ({ columns, data, loading, emptyMessage, onRowClick, rowKey }) => {
  const getKey = (row: Record<string, unknown>, index: number) =>
    rowKey && row[rowKey] != null ? String(row[rowKey]) : `row-${index}`;
  return (
    <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
      {/* Desktop table */}
      <div className="hidden md:block overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-slate-700/50 bg-slate-800/30">
              {columns.map((col) => (
                <th key={col.key} className={`px-4 py-3 font-medium text-slate-400 ${col.align === 'right' ? 'text-right' : col.align === 'center' ? 'text-center' : 'text-left'}`}>
                  {col.label}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {data.length === 0 && !loading ? (
              <tr><td colSpan={columns.length} className="px-4 py-12 text-center text-slate-400">{emptyMessage ?? 'No data'}</td></tr>
            ) : data.map((row, i) => (
              <tr key={getKey(row, i)} className={`border-b border-slate-700/30 table-row-hover ${onRowClick ? 'cursor-pointer' : ''}`} onClick={() => onRowClick?.(row)}>
                {columns.map((col) => (
                  <td key={col.key} className={`px-4 py-2.5 ${col.align === 'right' ? 'text-right' : col.align === 'center' ? 'text-center' : 'text-left'}`}>
                    {col.render ? col.render(row[col.key], row) : String(row[col.key] ?? '')}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Mobile cards */}
      <div className="md:hidden divide-y divide-slate-700/30">
        {data.length === 0 && !loading ? (
          <div className="px-4 py-12 text-center text-slate-400">{emptyMessage ?? 'No data'}</div>
        ) : data.map((row, i) => (
          <div key={getKey(row, i)} className="px-4 py-3 space-y-1" onClick={() => onRowClick?.(row)}>
            {columns.filter((c) => !c.hideOnMobile).map((col) => (
              <div key={col.key} className="flex items-center justify-between text-sm">
                <span className="text-slate-400">{col.label}</span>
                <span className="text-white">{col.render ? col.render(row[col.key], row) : String(row[col.key] ?? '')}</span>
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
};

export default ResponsiveTable;
