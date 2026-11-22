// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useRef, useEffect } from 'react';
import { Download } from 'lucide-react';

interface ExportButtonProps {
  data: Record<string, unknown>[];
  filename: string;
  columns?: string[];
}

function escapeCSVValue(value: unknown): string {
  const str = value == null ? '' : String(value);
  if (str.includes(',') || str.includes('"') || str.includes('\n')) {
    return `"${str.replace(/"/g, '""')}"`;
  }
  return str;
}

function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  document.body.removeChild(anchor);
  URL.revokeObjectURL(url);
}

const ExportButton: React.FC<ExportButtonProps> = ({ data, filename, columns }) => {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    if (open) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [open]);

  const getHeaders = (): string[] => {
    if (columns && columns.length > 0) return columns;
    if (data.length === 0) return [];
    const keys = new Set<string>();
    for (const row of data) {
      for (const key of Object.keys(row)) {
        keys.add(key);
      }
    }
    return Array.from(keys);
  };

  const exportCSV = () => {
    const headers = getHeaders();
    const headerLine = headers.map(escapeCSVValue).join(',');
    const rows = data.map((row) =>
      headers.map((h) => escapeCSVValue(row[h])).join(','),
    );
    const csv = [headerLine, ...rows].join('\n');
    const blob = new Blob([csv], { type: 'text/csv;charset=utf-8;' });
    downloadBlob(blob, `${filename}.csv`);
    setOpen(false);
  };

  const exportJSON = () => {
    const output = columns
      ? data.map((row) => {
          const filtered: Record<string, unknown> = {};
          for (const col of columns) {
            filtered[col] = row[col];
          }
          return filtered;
        })
      : data;
    const json = JSON.stringify(output, null, 2);
    const blob = new Blob([json], { type: 'application/json;charset=utf-8;' });
    downloadBlob(blob, `${filename}.json`);
    setOpen(false);
  };

  return (
    <div className="relative inline-block" ref={ref}>
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-slate-700/50 text-sm text-slate-400 hover:text-white hover:bg-slate-700/50 transition-colors"
        title="Export data"
      >
        <Download className="w-4 h-4" />
        <span>Export</span>
      </button>

      {open && (
        <div className="absolute right-0 mt-1 w-40 rounded-lg border border-slate-700/50 bg-slate-800/50 shadow-lg z-30 overflow-hidden animate-scale-in">
          <button
            onClick={exportCSV}
            className="w-full text-left px-4 py-2 text-sm text-white hover:bg-slate-700/50 transition-colors"
          >
            Export CSV
          </button>
          <button
            onClick={exportJSON}
            className="w-full text-left px-4 py-2 text-sm text-white hover:bg-slate-700/50 transition-colors border-t border-slate-700/50"
          >
            Export JSON
          </button>
        </div>
      )}
    </div>
  );
};

export default ExportButton;
