// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useCallback } from 'react';
import { Search, X, ChevronDown } from 'lucide-react';

interface FilterOption {
  value: string;
  label: string;
}

interface FilterDef {
  key: string;
  label: string;
  type: 'text' | 'select' | 'multi-select';
  options?: FilterOption[];
  placeholder?: string;
}

interface FilterBarProps {
  filters: FilterDef[];
  values: Record<string, string | string[]>;
  onChange: (key: string, value: string | string[]) => void;
  onClear: () => void;
}

const hasActiveFilters = (values: Record<string, string | string[]>): boolean =>
  Object.values(values).some((v) =>
    Array.isArray(v) ? v.length > 0 : v !== '',
  );

const TextFilter: React.FC<{
  filter: FilterDef;
  value: string;
  onChange: (val: string) => void;
}> = ({ filter, value, onChange }) => (
  <div className="relative">
    <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-slate-500" />
    <input
      type="text"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={filter.placeholder ?? filter.label}
      className="w-44 pl-8 pr-3 py-1.5 rounded-lg bg-slate-700/50 border border-slate-600/50 text-sm text-slate-200 placeholder:text-slate-500 focus:outline-none focus:ring-1 focus:ring-blue-500/50 focus:border-blue-500/50 transition-colors"
    />
  </div>
);

const SelectFilter: React.FC<{
  filter: FilterDef;
  value: string;
  onChange: (val: string) => void;
}> = ({ filter, value, onChange }) => (
  <div className="relative">
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className="appearance-none w-40 pl-3 pr-8 py-1.5 rounded-lg bg-slate-700/50 border border-slate-600/50 text-sm text-slate-200 focus:outline-none focus:ring-1 focus:ring-blue-500/50 focus:border-blue-500/50 transition-colors cursor-pointer"
    >
      <option value="">{filter.placeholder ?? `All ${filter.label}`}</option>
      {filter.options?.map((opt) => (
        <option key={opt.value} value={opt.value}>
          {opt.label}
        </option>
      ))}
    </select>
    <ChevronDown className="absolute right-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-slate-500 pointer-events-none" />
  </div>
);

const MultiSelectFilter: React.FC<{
  filter: FilterDef;
  value: string[];
  onChange: (val: string[]) => void;
}> = ({ filter, value, onChange }) => {
  const toggleOption = useCallback(
    (optVal: string) => {
      if (value.includes(optVal)) {
        onChange(value.filter((v) => v !== optVal));
      } else {
        onChange([...value, optVal]);
      }
    },
    [value, onChange],
  );

  const removeChip = useCallback(
    (optVal: string) => {
      onChange(value.filter((v) => v !== optVal));
    },
    [value, onChange],
  );

  return (
    <div className="flex items-center gap-1.5 flex-wrap">
      <div className="relative">
        <select
          value=""
          onChange={(e) => {
            if (e.target.value) toggleOption(e.target.value);
          }}
          className="appearance-none w-40 pl-3 pr-8 py-1.5 rounded-lg bg-slate-700/50 border border-slate-600/50 text-sm text-slate-200 focus:outline-none focus:ring-1 focus:ring-blue-500/50 focus:border-blue-500/50 transition-colors cursor-pointer"
        >
          <option value="">{filter.placeholder ?? `Add ${filter.label}`}</option>
          {filter.options
            ?.filter((opt) => !value.includes(opt.value))
            .map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
        </select>
        <ChevronDown className="absolute right-2.5 top-1/2 -translate-y-1/2 w-3.5 h-3.5 text-slate-500 pointer-events-none" />
      </div>

      {value.map((v) => {
        const opt = filter.options?.find((o) => o.value === v);
        return (
          <span
            key={v}
            className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full bg-blue-500/20 text-blue-400 border border-blue-500/30 text-xs font-medium"
          >
            {opt?.label ?? v}
            <button
              onClick={() => removeChip(v)}
              className="hover:text-blue-200 transition-colors"
              aria-label={`Remove ${opt?.label ?? v}`}
            >
              <X className="w-3 h-3" />
            </button>
          </span>
        );
      })}
    </div>
  );
};

const FilterBar: React.FC<FilterBarProps> = ({
  filters,
  values,
  onChange,
  onClear,
}) => {
  const active = hasActiveFilters(values);

  return (
    <div className="flex items-center gap-3 flex-wrap px-4 py-3 rounded-xl border border-slate-700/50 bg-slate-800/50">
      {filters.map((filter) => {
        const val = values[filter.key];

        if (filter.type === 'text') {
          return (
            <TextFilter
              key={filter.key}
              filter={filter}
              value={typeof val === 'string' ? val : ''}
              onChange={(v) => onChange(filter.key, v)}
            />
          );
        }

        if (filter.type === 'select') {
          return (
            <SelectFilter
              key={filter.key}
              filter={filter}
              value={typeof val === 'string' ? val : ''}
              onChange={(v) => onChange(filter.key, v)}
            />
          );
        }

        if (filter.type === 'multi-select') {
          return (
            <MultiSelectFilter
              key={filter.key}
              filter={filter}
              value={Array.isArray(val) ? val : []}
              onChange={(v) => onChange(filter.key, v)}
            />
          );
        }

        return null;
      })}

      {active && (
        <button
          onClick={onClear}
          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium text-slate-400 hover:text-white hover:bg-slate-700/50 transition-colors"
        >
          <X className="w-3.5 h-3.5" />
          Clear all
        </button>
      )}
    </div>
  );
};

export default FilterBar;
