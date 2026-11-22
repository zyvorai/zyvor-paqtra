// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';
import { Box, Shield, ChevronRight, X, Layers } from 'lucide-react';
import { useNamespaceStore } from '../stores/namespaceStore';

export interface NamespaceInfo {
  name: string;
  pods: number;
  endpoints: number;
  policies: number;
  status?: 'active' | 'warning' | 'error';
}

interface NamespaceSidebarProps {
  namespaces: NamespaceInfo[];
}

const STATUS_DOT: Record<string, string> = {
  active: 'bg-emerald-400',
  warning: 'bg-yellow-400',
  error: 'bg-red-400',
};

export const NamespaceSidebar: React.FC<NamespaceSidebarProps> = ({ namespaces }) => {
  const { selectedNamespace, setNamespace, clearNamespace } = useNamespaceStore();

  return (
    <div className="bg-slate-800/50 rounded-xl border border-slate-700/50 p-5 h-full">
      <h3 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-orange-500 to-orange-700 shadow-lg shadow-orange-500/20 flex items-center justify-center">
          <Box className="w-4 h-4 text-white" />
        </div>
        Namespaces
        <span className="ml-auto text-[10px] font-medium px-2 py-0.5 rounded-full bg-slate-700/50 text-slate-400">
          {namespaces.length}
        </span>
      </h3>

      {namespaces.length === 0 ? (
        <div className="text-center py-8 text-slate-500 text-sm">
          No namespaces found
        </div>
      ) : (
        <div className="space-y-2 overflow-y-auto max-h-[360px] pr-1">
          {/* All Namespaces option */}
          <div
            onClick={clearNamespace}
            className={`rounded-lg p-3 border transition-all cursor-pointer group ${
              selectedNamespace === ''
                ? 'bg-blue-500/10 border-l-4 border-l-blue-500 border-t-blue-500/30 border-r-blue-500/30 border-b-blue-500/30'
                : 'bg-slate-900/50 border-slate-700/30 hover:border-slate-600/50 hover:bg-slate-900/70'
            }`}
          >
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2 min-w-0">
                <Layers className={`w-4 h-4 flex-shrink-0 ${selectedNamespace === '' ? 'text-blue-400' : 'text-slate-500'}`} />
                <span className={`text-sm truncate ${selectedNamespace === '' ? 'font-bold text-blue-300' : 'font-medium text-slate-400'}`}>
                  All Namespaces
                </span>
              </div>
              <ChevronRight className={`w-3.5 h-3.5 flex-shrink-0 transition-colors ${selectedNamespace === '' ? 'text-blue-400' : 'text-slate-600 group-hover:text-slate-400'}`} />
            </div>
          </div>

          {namespaces.map((ns) => {
            const dotColor = STATUS_DOT[ns.status || 'active'] || STATUS_DOT.active;
            const isSelected = selectedNamespace === ns.name;
            return (
              <div
                key={ns.name}
                onClick={() => setNamespace(ns.name)}
                className={`rounded-lg p-3 border transition-all cursor-pointer group ${
                  isSelected
                    ? 'bg-blue-500/10 border-l-4 border-l-blue-500 border-t-blue-500/30 border-r-blue-500/30 border-b-blue-500/30'
                    : 'bg-slate-900/50 border-slate-700/30 hover:border-slate-600/50 hover:bg-slate-900/70'
                }`}
              >
                <div className="flex items-center justify-between mb-2">
                  <div className="flex items-center gap-2 min-w-0">
                    <span className={`flex-shrink-0 w-2 h-2 rounded-full ${dotColor}`} />
                    <span className={`text-sm truncate ${isSelected ? 'font-bold text-blue-300' : 'font-semibold text-white'}`}>
                      {ns.name}
                    </span>
                  </div>
                  <div className="flex items-center gap-1 flex-shrink-0">
                    {isSelected && (
                      <button
                        onClick={(e) => { e.stopPropagation(); clearNamespace(); }}
                        className="w-5 h-5 rounded flex items-center justify-center text-blue-400 hover:text-blue-300 hover:bg-blue-500/20 transition-colors"
                        title="Clear namespace filter"
                      >
                        <X className="w-3.5 h-3.5" />
                      </button>
                    )}
                    <ChevronRight className={`w-3.5 h-3.5 transition-colors ${isSelected ? 'text-blue-400' : 'text-slate-600 group-hover:text-slate-400'}`} />
                  </div>
                </div>
                <div className="grid grid-cols-3 gap-2">
                  <div className="flex flex-col items-center bg-slate-800/60 rounded-md py-1.5 px-1">
                    <span className="text-xs font-bold text-blue-400">{ns.pods}</span>
                    <span className="text-[10px] text-slate-500">Pods</span>
                  </div>
                  <div className="flex flex-col items-center bg-slate-800/60 rounded-md py-1.5 px-1">
                    <span className="text-xs font-bold text-cyan-400">{ns.endpoints}</span>
                    <span className="text-[10px] text-slate-500">Endpoints</span>
                  </div>
                  <div className="flex flex-col items-center bg-slate-800/60 rounded-md py-1.5 px-1">
                    <span className="text-xs font-bold text-purple-400">{ns.policies}</span>
                    <span className="text-[10px] text-slate-500">
                      <Shield className="w-3 h-3 inline-block" />
                    </span>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};

export default NamespaceSidebar;
