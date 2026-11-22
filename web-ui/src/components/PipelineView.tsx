// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';
import { ArrowRight, Activity } from 'lucide-react';

interface PipelineStage {
  name: string;
  count: number;
  color: string;
  active?: boolean;
}

interface PipelineViewProps {
  stages: PipelineStage[];
}

const COLOR_MAP: Record<string, { bg: string; border: string; text: string; glow: string; badge: string }> = {
  blue: {
    bg: 'bg-blue-500/10',
    border: 'border-blue-500/30',
    text: 'text-blue-400',
    glow: 'shadow-blue-500/20',
    badge: 'bg-blue-500/20 text-blue-300',
  },
  green: {
    bg: 'bg-emerald-500/10',
    border: 'border-emerald-500/30',
    text: 'text-emerald-400',
    glow: 'shadow-emerald-500/20',
    badge: 'bg-emerald-500/20 text-emerald-300',
  },
  red: {
    bg: 'bg-red-500/10',
    border: 'border-red-500/30',
    text: 'text-red-400',
    glow: 'shadow-red-500/20',
    badge: 'bg-red-500/20 text-red-300',
  },
  purple: {
    bg: 'bg-purple-500/10',
    border: 'border-purple-500/30',
    text: 'text-purple-400',
    glow: 'shadow-purple-500/20',
    badge: 'bg-purple-500/20 text-purple-300',
  },
  cyan: {
    bg: 'bg-cyan-500/10',
    border: 'border-cyan-500/30',
    text: 'text-cyan-400',
    glow: 'shadow-cyan-500/20',
    badge: 'bg-cyan-500/20 text-cyan-300',
  },
  orange: {
    bg: 'bg-orange-500/10',
    border: 'border-orange-500/30',
    text: 'text-orange-400',
    glow: 'shadow-orange-500/20',
    badge: 'bg-orange-500/20 text-orange-300',
  },
  yellow: {
    bg: 'bg-yellow-500/10',
    border: 'border-yellow-500/30',
    text: 'text-yellow-400',
    glow: 'shadow-yellow-500/20',
    badge: 'bg-yellow-500/20 text-yellow-300',
  },
};

const DEFAULT_COLORS = COLOR_MAP.blue;

function getColorConfig(color: string) {
  return COLOR_MAP[color] || DEFAULT_COLORS;
}

export const PipelineView: React.FC<PipelineViewProps> = ({ stages }) => {
  return (
    <div className="bg-slate-800/50 rounded-xl border border-slate-700/50 p-5">
      <h3 className="text-sm font-semibold text-white mb-5 flex items-center gap-2">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-cyan-500 to-cyan-700 shadow-lg shadow-cyan-500/20 flex items-center justify-center">
          <Activity className="w-4 h-4 text-white" />
        </div>
        Flow Pipeline
      </h3>
      <div className="flex items-center gap-1 overflow-x-auto pb-2">
        {stages.map((stage, index) => {
          const colors = getColorConfig(stage.color);
          return (
            <React.Fragment key={stage.name}>
              <div
                className={`
                  flex-shrink-0 rounded-xl px-5 py-4 border transition-all
                  ${colors.bg} ${colors.border}
                  ${stage.active ? `shadow-lg ${colors.glow} scale-[1.03]` : ''}
                `}
              >
                <div className="flex flex-col items-center gap-2">
                  <span className={`text-xl font-bold ${colors.text}`}>
                    {stage.count.toLocaleString()}
                  </span>
                  <span className="text-xs text-slate-400 font-medium whitespace-nowrap">
                    {stage.name}
                  </span>
                  {stage.active && (
                    <span className={`text-[10px] font-medium px-2 py-0.5 rounded-full ${colors.badge}`}>
                      Active
                    </span>
                  )}
                </div>
              </div>
              {index < stages.length - 1 && (
                <div className="flex-shrink-0 flex items-center px-1">
                  <ArrowRight
                    className={`w-5 h-5 text-slate-600 ${
                      stage.active ? 'animate-pulse text-slate-400' : ''
                    }`}
                  />
                </div>
              )}
            </React.Fragment>
          );
        })}
      </div>
    </div>
  );
};

export default PipelineView;
