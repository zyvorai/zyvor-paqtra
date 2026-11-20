// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react';
import { Server } from 'lucide-react';

interface SystemInfoItem {
  label: string;
  value: string;
  icon?: React.ReactNode;
}

interface SystemInfoPanelProps {
  info: SystemInfoItem[];
  title?: string;
}

export const SystemInfoPanel: React.FC<SystemInfoPanelProps> = ({
  info,
  title = 'System Information',
}) => {
  return (
    <div className="bg-slate-800/50 rounded-xl border border-slate-700/50 p-5">
      <h3 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
        <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-purple-500 to-purple-700 shadow-lg shadow-purple-500/20 flex items-center justify-center">
          <Server className="w-4 h-4 text-white" />
        </div>
        {title}
      </h3>
      <div className="divide-y divide-slate-700/40">
        {info.map((item, index) => (
          <div
            key={index}
            className="flex items-center justify-between py-2.5 first:pt-0 last:pb-0"
          >
            <div className="flex items-center gap-2 text-slate-400">
              {item.icon && (
                <span className="w-4 h-4 flex items-center justify-center text-slate-500">
                  {item.icon}
                </span>
              )}
              <span className="text-xs">{item.label}</span>
            </div>
            <span className="text-sm font-medium text-white truncate ml-4 max-w-[60%] text-right">
              {item.value}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
};

export default SystemInfoPanel;
