// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState } from 'react';
import { ChevronDown } from 'lucide-react';

interface AccordionProps {
  title: string;
  icon?: React.ReactNode;
  defaultOpen?: boolean;
  children: React.ReactNode;
}

const Accordion: React.FC<AccordionProps> = ({ title, icon, defaultOpen = false, children }) => {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
      <button
        onClick={() => setOpen(!open)}
        className="w-full flex items-center justify-between px-5 py-4 text-left hover:bg-slate-700/20 transition-colors"
      >
        <div className="flex items-center gap-3">
          {icon && <span className="text-slate-400">{icon}</span>}
          <span className="text-sm font-semibold text-white">{title}</span>
        </div>
        <ChevronDown className={`w-4 h-4 text-slate-400 transition-transform ${open ? 'rotate-180' : ''}`} />
      </button>
      {open && (
        <div className="px-5 pb-5 animate-fade-in">
          {children}
        </div>
      )}
    </div>
  );
};

export default Accordion;
