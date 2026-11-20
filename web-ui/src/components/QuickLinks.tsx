// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React from 'react';
import { useNavigate } from 'react-router-dom';

interface QuickLink {
  title: string;
  description: string;
  icon: React.ReactNode;
  path: string;
}

interface QuickLinksProps {
  links: QuickLink[];
}

export const QuickLinks: React.FC<QuickLinksProps> = ({ links }) => {
  const navigate = useNavigate();

  return (
    <div className="mb-6">
      <h2 className="text-sm font-semibold text-slate-400 uppercase tracking-wider mb-3">
        Quick links
      </h2>
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-6 gap-3">
        {links.map((link, index) => (
          <button
            key={index}
            onClick={() => navigate(link.path)}
            className="bg-slate-800/50 border border-slate-700/50 rounded-xl p-4 text-left transition-all hover:border-blue-500/50 hover:scale-[1.02] hover:shadow-lg hover:shadow-blue-500/5 card-glow"
          >
            <div className="mb-2 text-slate-400">
              {link.icon}
            </div>
            <h3 className="text-xs font-semibold text-white mb-1">
              {link.title}
            </h3>
            <p className="text-[10px] text-slate-500 leading-relaxed">
              {link.description}
            </p>
          </button>
        ))}
      </div>
    </div>
  );
};

export default QuickLinks;
