// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState } from 'react';
import { Stethoscope, Play, Loader2, CheckCircle, XCircle, AlertTriangle } from 'lucide-react';
import { runDiagnostics, DiagnosticTest } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { countTone } from '../../components/kit/tone';

const STATUS_ICON: Record<string, React.ReactNode> = {
  pass: <CheckCircle className="w-5 h-5 text-green-400" />,
  fail: <XCircle className="w-5 h-5 text-red-400" />,
  warn: <AlertTriangle className="w-5 h-5 text-yellow-400" />,
  running: <Loader2 className="w-5 h-5 animate-spin text-blue-400" />,
};

const STATUS_BG: Record<string, string> = {
  pass: 'border-l-green-400', fail: 'border-l-red-400', warn: 'border-l-yellow-400', running: 'border-l-blue-400',
};

const Diagnostics: React.FC = () => {
  usePageTitle('Diagnostics');
  const [tests, setTests] = useState<DiagnosticTest[]>([]);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleRun = async () => {
    setRunning(true); setError(null); setTests([]);
    try { setTests((await runDiagnostics()).data.tests ?? []); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Diagnostics failed'); }
    finally { setRunning(false); }
  };

  const pass = tests.filter((t) => t.status === 'pass').length;
  const fail = tests.filter((t) => t.status === 'fail').length;
  const warn = tests.filter((t) => t.status === 'warn').length;

  const tick = useChanged(tests);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-purple-500 to-purple-700 flex items-center justify-center shadow-lg shadow-purple-500/20"><Stethoscope className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Network Diagnostics</h1></div>
          <p className="text-sm text-slate-400 mt-1">Connectivity tests, DNS checks, and health validation</p>
        </div>
        <button onClick={handleRun} disabled={running}
          className="flex items-center gap-2 px-4 py-2 rounded-lg bg-gradient-to-r from-blue-600 to-blue-700 text-white text-sm hover:from-blue-500 hover:to-blue-600 disabled:opacity-50 transition-colors">
          {running ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4" />}
          {running ? 'Running...' : 'Run Diagnostics'}
        </button>
      </div>

      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (tests.length ? (fail ? `${fail} of ${tests.length} checks failed.` : `All ${tests.length} checks passed.`) : undefined) : undefined}
        tone={tick ? (fail ? 'bad' : warn ? 'warn' : undefined) : undefined}
        tick={tick}
        live={false}
        error={error || undefined}
        figures={[
          { label: 'passed', value: tick ? pass : undefined },
          { label: 'failed', value: tick ? fail : undefined, tone: tick ? (countTone(fail)) : undefined },
          { label: 'warnings', value: tick ? warn : undefined, tone: tick ? (countTone(warn)) : undefined },
        ]}
      />

      {tests.length > 0 && (
        <>

          <div className="space-y-2">
            {tests.map((t, i) => (
              <div key={i} className={`rounded-xl border border-slate-700/50 bg-slate-800/50 p-4 border-l-4 ${STATUS_BG[t.status]} animate-fade-in`} style={{ animationDelay: `${i * 50}ms` }}>
                <div className="flex items-center gap-3">
                  {STATUS_ICON[t.status]}
                  <div className="flex-1">
                    <div className="font-medium text-white">{t.name}</div>
                    <div className="text-sm text-slate-400">{t.message}</div>
                  </div>
                  <span className="text-xs text-slate-400">{t.duration_ms}ms</span>
                </div>
              </div>
            ))}
          </div>
        </>
      )}

      {tests.length === 0 && !running && (
        <div className="text-center py-16">
          <Stethoscope className="w-16 h-16 text-slate-400 mx-auto mb-4" />
          <h3 className="text-lg font-medium text-white mb-2">Ready to diagnose</h3>
          <p className="text-slate-400 mb-6">Run diagnostics to check Cilium agent health, connectivity, DNS, encryption, and more.</p>
        </div>
      )}
    </div>
  );
};

export default Diagnostics;
