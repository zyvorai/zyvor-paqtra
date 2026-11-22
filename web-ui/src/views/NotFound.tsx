// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React from 'react';
import { SearchX } from 'lucide-react';
import { useNavigate } from 'react-router-dom';

const NotFound: React.FC = () => {
  const navigate = useNavigate();

  return (
    <div className="flex items-center justify-center min-h-[60vh]">
      <div className="text-center max-w-md">
        <div className="w-20 h-20 rounded-2xl bg-gradient-to-br from-slate-600 to-slate-800 flex items-center justify-center mx-auto mb-6 shadow-lg">
          <SearchX className="w-10 h-10 text-white" />
        </div>
        <h1 className="text-5xl font-bold text-gradient-blue mb-2">404</h1>
        <p className="text-slate-400 mb-8">
          Page not found. The page you are looking for does not exist.
        </p>
        <button
          onClick={() => navigate('/')}
          className="px-6 py-2.5 rounded-lg bg-gradient-to-r from-blue-600 to-blue-700 text-white font-medium hover:from-blue-500 hover:to-blue-600 transition-all hover:scale-[1.02]"
        >
          Go to Dashboard
        </button>
      </div>
    </div>
  );
};

export default NotFound;
