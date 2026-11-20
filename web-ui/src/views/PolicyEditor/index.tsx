// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import React, { useState, useRef } from 'react';
import { FileEdit, CheckCircle, XCircle, Play, Loader2, Copy, RotateCcw } from 'lucide-react';
import Editor, { OnMount } from '@monaco-editor/react';
import { validatePolicy, createPolicy } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const DEFAULT_YAML = `apiVersion: cilium.io/v2
kind: CiliumNetworkPolicy
metadata:
  name: my-policy
  namespace: default
spec:
  endpointSelector:
    matchLabels:
      app: my-app
  ingress:
  - fromEndpoints:
    - matchLabels:
        app: frontend
    toPorts:
    - ports:
      - port: "8080"
        protocol: TCP
  egress:
  - toEndpoints:
    - matchLabels:
        k8s:io.kubernetes.pod.namespace: kube-system
        k8s-app: kube-dns
    toPorts:
    - ports:
      - port: "53"
        protocol: UDP`;

const PolicyEditor: React.FC = () => {
  usePageTitle('Policy Editor');
  const [yaml, setYaml] = useState(DEFAULT_YAML);
  const [validating, setValidating] = useState(false);
  const [applying, setApplying] = useState(false);
  const [validation, setValidation] = useState<{ valid: boolean; errors: string[] } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);
  const editorRef = useRef<Parameters<OnMount>[0] | null>(null);

  const handleEditorMount: OnMount = (editor) => {
    editorRef.current = editor;
  };

  const handleValidate = async () => {
    setValidating(true); setError(null); setValidation(null);
    try { setValidation((await validatePolicy(yaml)).data); }
    catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Validation failed'); }
    finally { setValidating(false); }
  };

  const handleApply = async () => {
    setApplying(true); setError(null); setSuccess(null);
    try {
      await createPolicy({ name: 'editor-policy', namespace: 'default', spec: yaml });
      setSuccess('Policy applied successfully');
    } catch (err) { setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Apply failed'); }
    finally { setApplying(false); }
  };

  const handleCopy = () => {
    navigator.clipboard.writeText(yaml);
    setSuccess('Copied to clipboard');
    setTimeout(() => setSuccess(null), 2000);
  };

  const handleReset = () => {
    setYaml(DEFAULT_YAML);
    setValidation(null);
    editorRef.current?.setValue(DEFAULT_YAML);
  };

  const lineCount = yaml.split('\n').length;

  const tick = useChanged(validation);
  return (
    <div className="netra-page">
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3"><div className="w-10 h-10 rounded-lg bg-gradient-to-br from-blue-500 to-blue-700 flex items-center justify-center shadow-lg shadow-blue-500/20"><FileEdit className="w-5 h-5 text-white" /></div><h1 className="text-2xl font-bold text-white">Policy Editor</h1></div>
          <p className="text-sm text-slate-400 mt-1">Write, validate, and apply CiliumNetworkPolicies with syntax highlighting</p>
        </div>
      </div>
      {error && <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-sm">{error}</div>}
      <PagePulse
        headline={tick ? (validation ? (validation.valid ? 'Policy is valid — review, then apply.' : `${validation.errors.length} validation error${validation.errors.length === 1 ? '' : 's'}.`) : undefined) : undefined}
        tone={tick ? (validation && !validation.valid ? 'bad' : undefined) : undefined}
        tick={tick}
        live={false}
        error={error || undefined}
        figures={[
          { label: 'validation', value: tick ? (validation ? (validation.valid ? 'valid' : 'invalid') : '—') : undefined, tone: tick ? ((validation ? (validation.valid ? 'ok' : 'bad') : undefined)) : undefined },
          { label: 'YAML lines', value: tick ? yaml.split("\n").length : undefined },
        ]}
      />
      {success && <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 text-green-400 text-sm">{success}</div>}

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        {/* Monaco Editor */}
        <div className="lg:col-span-2 rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden">
          <div className="flex items-center justify-between px-4 py-2 border-b border-slate-700/50 bg-slate-900/95">
            <div className="flex items-center gap-3">
              <div className="flex items-center gap-1.5">
                <span className="w-3 h-3 rounded-full bg-red-500/80" />
                <span className="w-3 h-3 rounded-full bg-yellow-500/80" />
                <span className="w-3 h-3 rounded-full bg-green-500/80" />
              </div>
              <h2 className="text-sm font-semibold text-white">YAML Editor</h2>
            </div>
            <div className="flex gap-2">
              <button onClick={handleCopy} className="flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-slate-700/50 text-xs text-slate-400 hover:text-white hover:bg-slate-700/30 transition-colors">
                <Copy className="w-3.5 h-3.5" /> Copy
              </button>
              <button onClick={handleReset} className="flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-slate-700/50 text-xs text-slate-400 hover:text-white hover:bg-slate-700/30 transition-colors">
                <RotateCcw className="w-3.5 h-3.5" /> Reset
              </button>
              <button onClick={handleValidate} disabled={validating} className="flex items-center gap-1.5 px-2.5 py-1 rounded-md border border-slate-700/50 text-xs hover:bg-slate-700/30 disabled:opacity-50 transition-colors">
                {validating ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <CheckCircle className="w-3.5 h-3.5" />} Validate
              </button>
              <button onClick={handleApply} disabled={applying} className="flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-blue-600 text-white text-xs hover:bg-blue-600/90 disabled:opacity-50 transition-colors">
                {applying ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Play className="w-3.5 h-3.5" />} Apply
              </button>
            </div>
          </div>
          <Editor
            height="600px"
            defaultLanguage="yaml"
            value={yaml}
            onChange={(v) => setYaml(v ?? '')}
            onMount={handleEditorMount}
            theme="vs-dark"
            options={{
              minimap: { enabled: false },
              fontSize: 13,
              lineNumbers: 'on',
              scrollBeyondLastLine: false,
              wordWrap: 'on',
              tabSize: 2,
              automaticLayout: true,
              padding: { top: 12 },
              renderLineHighlight: 'line',
              scrollbar: { verticalScrollbarSize: 8, horizontalScrollbarSize: 8 },
            }}
          />
          <div className="px-4 py-1.5 border-t border-slate-700/50 text-xs text-slate-400 flex items-center gap-4">
            <span>Lines: {lineCount}</span>
            <span>Characters: {yaml.length}</span>
            <span>Language: YAML</span>
          </div>
        </div>

        {/* Sidebar */}
        <div className="space-y-4">
          {/* Validation result */}
          {validation && (
            <div className={`rounded-xl border p-4 animate-scale-in ${validation.valid ? 'border-green-500/30 bg-green-500/5' : 'border-red-500/30 bg-red-500/5'}`}>
              <div className="flex items-center gap-2 mb-2">
                {validation.valid ? <CheckCircle className="w-5 h-5 text-green-400" /> : <XCircle className="w-5 h-5 text-red-400" />}
                <span className={`font-semibold ${validation.valid ? 'text-green-400' : 'text-red-400'}`}>{validation.valid ? 'Valid Policy' : 'Validation Errors'}</span>
              </div>
              {validation.valid && <p className="text-sm text-green-400/80">The policy YAML is syntactically correct and ready to apply.</p>}
              {validation.errors.length > 0 && (
                <ul className="space-y-1 text-sm text-red-400">
                  {validation.errors.map((e, i) => <li key={i} className="flex items-start gap-1"><span className="mt-0.5">&bull;</span>{e}</li>)}
                </ul>
              )}
            </div>
          )}

          {/* Quick reference */}
          <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4">
            <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><FileEdit className="w-4 h-4 text-blue-400" /> CiliumNetworkPolicy Reference</h3>
            <div className="space-y-2.5 text-xs">
              {[
                ['endpointSelector', 'Select pods this policy applies to'],
                ['ingress', 'Allow inbound traffic rules'],
                ['egress', 'Allow outbound traffic rules'],
                ['fromEndpoints', 'Match source by K8s labels'],
                ['toEndpoints', 'Match destination by K8s labels'],
                ['toPorts', 'Match by port and protocol'],
                ['toCIDR', 'Match external IP ranges'],
                ['toFQDNs', 'Match by DNS name (L7)'],
                ['fromCIDR', 'Match source IP ranges'],
                ['toServices', 'Match K8s services by name'],
                ['toGroups', 'Match cloud provider groups (AWS, etc.)'],
              ].map(([key, desc]) => (
                <div key={key}>
                  <span className="text-blue-400 font-mono font-medium">{key}</span>
                  <span className="text-slate-400 ml-2">{desc}</span>
                </div>
              ))}
            </div>
          </div>

          {/* Snippets */}
          <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 p-4">
            <h3 className="text-sm font-semibold text-white mb-3 flex items-center gap-2"><Copy className="w-4 h-4 text-purple-400" /> Quick Snippets</h3>
            <div className="space-y-2">
              {[
                { label: 'Allow DNS', snippet: '  - toEndpoints:\n    - matchLabels:\n        k8s:io.kubernetes.pod.namespace: kube-system\n        k8s-app: kube-dns\n    toPorts:\n    - ports:\n      - port: "53"\n        protocol: UDP' },
                { label: 'Allow HTTP', snippet: '  - toPorts:\n    - ports:\n      - port: "80"\n      - port: "443"' },
                { label: 'Allow from namespace', snippet: '  - fromEndpoints:\n    - matchLabels:\n        io.kubernetes.pod.namespace: monitoring' },
              ].map((s) => (
                <button key={s.label} onClick={() => { navigator.clipboard.writeText(s.snippet); setSuccess(`Copied: ${s.label}`); setTimeout(() => setSuccess(null), 1500); }}
                  className="w-full text-left px-3 py-2 rounded-lg border border-slate-700/50 text-xs hover:bg-slate-700/30 transition-colors">
                  <span className="text-white font-medium">{s.label}</span>
                  <span className="text-slate-400 ml-2">Click to copy</span>
                </button>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default PolicyEditor;
