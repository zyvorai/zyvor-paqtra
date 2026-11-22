// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import React, { useState, useMemo, useCallback } from 'react';
import {
  Shield, Plus, Trash2, Copy, Play, CheckCircle, XCircle,
  Loader2, RotateCcw, FlaskConical, ArrowRight,
} from 'lucide-react';
import { createPolicy, validatePolicy, simulatePolicy } from '../../services/api';
import { isAxiosError } from 'axios';
import { usePageTitle } from '../../hooks/usePageTitle';
import { useAutoDismiss } from '../../hooks/useAutoDismiss';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

/* ------------------------------------------------------------------ */
/*  Types                                                              */
/* ------------------------------------------------------------------ */

interface LabelPair {
  key: string;
  value: string;
}

type IngressSourceType = 'Labels' | 'CIDR' | 'Any';
type EgressDestType = 'Labels' | 'CIDR' | 'FQDN' | 'Service' | 'Any';

interface PortEntry {
  port: string;
  protocol: 'TCP' | 'UDP' | 'SCTP' | 'Any';
}

interface IngressRule {
  sourceType: IngressSourceType;
  labels: LabelPair[];
  cidr: string;
  ports: PortEntry[];
}

interface EgressRule {
  destType: EgressDestType;
  labels: LabelPair[];
  cidr: string;
  fqdn: string;
  serviceName: string;
  serviceNamespace: string;
  ports: PortEntry[];
}

interface FormState {
  name: string;
  namespace: string;
  description: string;
  endpointLabels: LabelPair[];
  ingress: IngressRule[];
  egress: EgressRule[];
}

/* ------------------------------------------------------------------ */
/*  Defaults                                                           */
/* ------------------------------------------------------------------ */

const defaultLabel = (): LabelPair => ({ key: '', value: '' });
const defaultPort = (): PortEntry => ({ port: '', protocol: 'TCP' });

const defaultIngressRule = (): IngressRule => ({
  sourceType: 'Labels',
  labels: [defaultLabel()],
  cidr: '',
  ports: [defaultPort()],
});

const defaultEgressRule = (): EgressRule => ({
  destType: 'Labels',
  labels: [defaultLabel()],
  cidr: '',
  fqdn: '',
  serviceName: '',
  serviceNamespace: '',
  ports: [defaultPort()],
});

const defaultForm = (): FormState => ({
  name: 'my-policy',
  namespace: 'default',
  description: '',
  endpointLabels: [{ key: 'app', value: 'my-app' }],
  ingress: [defaultIngressRule()],
  egress: [],
});

/* ------------------------------------------------------------------ */
/*  YAML generator                                                     */
/* ------------------------------------------------------------------ */

function portsToYaml(ports: PortEntry[], baseIndent: number): string {
  const valid = ports.filter((p) => p.port.trim());
  if (valid.length === 0) return '';
  const pad = ' '.repeat(baseIndent);
  let out = `${pad}toPorts:\n${pad}- ports:\n`;
  out += valid
    .map((p) => {
      let entry = `${pad}  - port: "${p.port}"`;
      if (p.protocol !== 'Any') entry += `\n${pad}    protocol: ${p.protocol}`;
      return entry;
    })
    .join('\n');
  return out;
}

function generateYAML(form: FormState): string {
  const lines: string[] = [];

  lines.push('apiVersion: cilium.io/v2');
  lines.push('kind: CiliumNetworkPolicy');
  lines.push('metadata:');
  lines.push(`  name: ${form.name || 'my-policy'}`);
  lines.push(`  namespace: ${form.namespace || 'default'}`);
  if (form.description.trim()) {
    lines.push('  annotations:');
    lines.push(`    description: "${form.description}"`);
  }
  lines.push('spec:');

  // Endpoint selector
  const epLabels = form.endpointLabels.filter((l) => l.key.trim());
  if (epLabels.length > 0) {
    lines.push('  endpointSelector:');
    lines.push('    matchLabels:');
    epLabels.forEach((l) => lines.push(`      ${l.key}: ${l.value}`));
  } else {
    lines.push('  endpointSelector: {}');
  }

  // Ingress
  if (form.ingress.length > 0) {
    lines.push('  ingress:');
    form.ingress.forEach((rule) => {
      const parts: string[] = [];

      if (rule.sourceType === 'Labels') {
        const valid = rule.labels.filter((l) => l.key.trim());
        if (valid.length > 0) {
          parts.push('  - fromEndpoints:');
          parts.push('    - matchLabels:');
          valid.forEach((l) => parts.push(`        ${l.key}: ${l.value}`));
        } else {
          parts.push('  - fromEndpoints:');
          parts.push('    - matchLabels: {}');
        }
      } else if (rule.sourceType === 'CIDR') {
        parts.push('  - fromCIDR:');
        if (rule.cidr.trim()) parts.push(`    - ${rule.cidr.trim()}`);
      } else {
        parts.push('  - {}');
      }

      const portsYaml = portsToYaml(rule.ports, 4);
      if (portsYaml) parts.push(portsYaml);

      lines.push(...parts);
    });
  }

  // Egress
  if (form.egress.length > 0) {
    lines.push('  egress:');
    form.egress.forEach((rule) => {
      const parts: string[] = [];

      switch (rule.destType) {
        case 'Labels': {
          const valid = rule.labels.filter((l) => l.key.trim());
          if (valid.length > 0) {
            parts.push('  - toEndpoints:');
            parts.push('    - matchLabels:');
            valid.forEach((l) => parts.push(`        ${l.key}: ${l.value}`));
          } else {
            parts.push('  - toEndpoints:');
            parts.push('    - matchLabels: {}');
          }
          break;
        }
        case 'CIDR':
          parts.push('  - toCIDR:');
          if (rule.cidr.trim()) parts.push(`    - ${rule.cidr.trim()}`);
          break;
        case 'FQDN':
          parts.push('  - toFQDNs:');
          if (rule.fqdn.trim()) parts.push(`    - matchPattern: "${rule.fqdn.trim()}"`);
          break;
        case 'Service':
          parts.push('  - toServices:');
          parts.push('    - k8sService:');
          if (rule.serviceName.trim()) parts.push(`        serviceName: ${rule.serviceName.trim()}`);
          if (rule.serviceNamespace.trim()) parts.push(`        namespace: ${rule.serviceNamespace.trim()}`);
          break;
        case 'Any':
          parts.push('  - {}');
          break;
      }

      const portsYaml = portsToYaml(rule.ports, 4);
      if (portsYaml) parts.push(portsYaml);

      lines.push(...parts);
    });
  }

  return lines.join('\n') + '\n';
}

/* ------------------------------------------------------------------ */
/*  Reusable sub-components                                            */
/* ------------------------------------------------------------------ */

const sectionCls =
  'rounded-xl border border-slate-700/50 bg-slate-800/50 p-5';
const inputCls =
  'w-full rounded-lg border border-slate-700/50 bg-slate-900/60 px-3 py-2 text-sm text-white placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500/40 focus:border-blue-500/60 transition-colors';
const btnSmCls =
  'flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-xs font-medium transition-colors';
const radioCls =
  'w-4 h-4 text-blue-500 bg-slate-900 border-slate-600 focus:ring-blue-500/40 focus:ring-offset-0 accent-blue-500';

interface LabelListProps {
  labels: LabelPair[];
  onChange: (labels: LabelPair[]) => void;
}

const LabelList: React.FC<LabelListProps> = ({ labels, onChange }) => (
  <div className="space-y-2">
    {labels.map((lbl, i) => (
      <div key={i} className="flex items-center gap-2">
        <input
          className={inputCls}
          placeholder="Key"
          value={lbl.key}
          onChange={(e) => {
            const next = [...labels];
            next[i] = { ...next[i], key: e.target.value };
            onChange(next);
          }}
        />
        <span className="text-slate-500 text-sm">=</span>
        <input
          className={inputCls}
          placeholder="Value"
          value={lbl.value}
          onChange={(e) => {
            const next = [...labels];
            next[i] = { ...next[i], value: e.target.value };
            onChange(next);
          }}
        />
        {labels.length > 1 && (
          <button
            onClick={() => onChange(labels.filter((_, j) => j !== i))}
            className="text-slate-500 hover:text-red-400 transition-colors p-1"
            title="Remove label"
          >
            <Trash2 className="w-4 h-4" />
          </button>
        )}
      </div>
    ))}
    <button
      onClick={() => onChange([...labels, defaultLabel()])}
      className={`${btnSmCls} text-blue-400 hover:text-blue-300 hover:bg-blue-500/10 border border-dashed border-slate-700/50`}
    >
      <Plus className="w-3.5 h-3.5" /> Add Label
    </button>
  </div>
);

interface PortListProps {
  ports: PortEntry[];
  onChange: (ports: PortEntry[]) => void;
}

const PortList: React.FC<PortListProps> = ({ ports, onChange }) => (
  <div className="space-y-2">
    <label className="text-xs font-medium text-slate-400 uppercase tracking-wider">Ports</label>
    {ports.map((p, i) => (
      <div key={i} className="flex items-center gap-2">
        <input
          className={inputCls}
          placeholder="Port (e.g. 8080)"
          value={p.port}
          onChange={(e) => {
            const next = [...ports];
            next[i] = { ...next[i], port: e.target.value };
            onChange(next);
          }}
        />
        <select
          className={`${inputCls} w-28 shrink-0`}
          value={p.protocol}
          onChange={(e) => {
            const next = [...ports];
            next[i] = { ...next[i], protocol: e.target.value as PortEntry['protocol'] };
            onChange(next);
          }}
        >
          <option value="TCP">TCP</option>
          <option value="UDP">UDP</option>
          <option value="SCTP">SCTP</option>
          <option value="Any">Any</option>
        </select>
        {ports.length > 1 && (
          <button
            onClick={() => onChange(ports.filter((_, j) => j !== i))}
            className="text-slate-500 hover:text-red-400 transition-colors p-1"
            title="Remove port"
          >
            <Trash2 className="w-4 h-4" />
          </button>
        )}
      </div>
    ))}
    <button
      onClick={() => onChange([...ports, defaultPort()])}
      className={`${btnSmCls} text-blue-400 hover:text-blue-300 hover:bg-blue-500/10 border border-dashed border-slate-700/50`}
    >
      <Plus className="w-3.5 h-3.5" /> Add Port
    </button>
  </div>
);

/* ------------------------------------------------------------------ */
/*  Main component                                                     */
/* ------------------------------------------------------------------ */

const RuleBuilder: React.FC = () => {
  usePageTitle('Visual Rule Builder');

  const [form, setForm] = useState<FormState>(defaultForm);
  const [validating, setValidating] = useState(false);
  const [applying, setApplying] = useState(false);
  const [simulating, setSimulating] = useState(false);
  const [validation, setValidation] = useState<{ valid: boolean; errors: string[] } | null>(null);
  const [simulationResult, setSimulationResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useAutoDismiss<string | null>(null);

  const yaml = useMemo(() => generateYAML(form), [form]);

  /* ---- helpers to update nested state ---- */

  const updateField = useCallback(
    <K extends keyof FormState>(key: K, value: FormState[K]) =>
      setForm((prev) => ({ ...prev, [key]: value })),
    [],
  );

  const updateIngress = useCallback(
    (index: number, patch: Partial<IngressRule>) =>
      setForm((prev) => ({
        ...prev,
        ingress: prev.ingress.map((r, i) => (i === index ? { ...r, ...patch } : r)),
      })),
    [],
  );

  const updateEgress = useCallback(
    (index: number, patch: Partial<EgressRule>) =>
      setForm((prev) => ({
        ...prev,
        egress: prev.egress.map((r, i) => (i === index ? { ...r, ...patch } : r)),
      })),
    [],
  );

  /* ---- actions ---- */

  const handleValidate = async (): Promise<boolean> => {
    setValidating(true);
    setError(null);
    setValidation(null);
    try {
      const res = await validatePolicy(yaml);
      setValidation(res.data);
      return res.data?.valid === true;
    } catch (err) {
      setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Validation failed');
      return false;
    } finally {
      setValidating(false);
    }
  };

  const handleApply = async () => {
    setApplying(true);
    setError(null);
    setSuccess(null);
    try {
      await createPolicy({
        name: form.name || 'my-policy',
        namespace: form.namespace || 'default',
        spec: yaml,
      });
      setSuccess('Policy applied successfully');
    } catch (err) {
      setError(isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Apply failed');
    } finally {
      setApplying(false);
    }
  };

  const handleSimulate = async () => {
    setSimulating(true);
    setError(null);
    setSimulationResult(null);
    try {
      const res = await simulatePolicy({
        name: form.name || 'my-policy',
        namespace: form.namespace || 'default',
        spec: yaml,
      });
      setSimulationResult(
        typeof res.data === 'string' ? res.data : JSON.stringify(res.data, null, 2),
      );
    } catch (err) {
      setError(
        isAxiosError(err) ? err.response?.data?.message ?? err.message : 'Simulation failed',
      );
    } finally {
      setSimulating(false);
    }
  };

  const handleCopy = () => {
    navigator.clipboard.writeText(yaml);
    setSuccess('YAML copied to clipboard');
  };

  const handleReset = () => {
    setForm(defaultForm());
    setValidation(null);
    setSimulationResult(null);
    setError(null);
  };

  /* ---- render ---- */

  const ingressSourceTypes: IngressSourceType[] = ['Labels', 'CIDR', 'Any'];
  const egressDestTypes: EgressDestType[] = ['Labels', 'CIDR', 'FQDN', 'Service', 'Any'];

  const tick = useChanged(validation);

  return (
    <div className="netra-page">
      {/* Header */}
      <div className="page-chrome flex items-center justify-between mb-6">
        <div>
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-lg bg-gradient-to-br from-purple-500 to-indigo-700 flex items-center justify-center shadow-lg shadow-purple-500/20">
              <Shield className="w-5 h-5 text-white" />
            </div>
            <h1 className="text-2xl font-bold text-white">Visual Rule Builder</h1>
          </div>
          <p className="text-sm text-slate-400 mt-1">
            Build CiliumNetworkPolicies visually — no YAML knowledge required
          </p>
        </div>
      </div>

      <PagePulse
        headline={
          tick
            ? validation
              ? validation.valid
                ? 'Policy is valid — review, then apply.'
                : `${validation.errors.length} validation error${validation.errors.length === 1 ? '' : 's'}.`
              : `${form.ingress.length} ingress, ${form.egress.length} egress rule${form.ingress.length + form.egress.length === 1 ? '' : 's'} drafted.`
            : undefined
        }
        tone={tick ? (validation && !validation.valid ? 'bad' : undefined) : undefined}
        tick={tick}
        live={false}
        error={error || undefined}
        figures={[
          { label: 'ingress rules', value: form.ingress.length },
          { label: 'egress rules', value: form.egress.length },
          {
            label: 'validation',
            value: tick ? (validation ? (validation.valid ? 'valid' : 'invalid') : '—') : undefined,
            tone: tick ? (validation ? (validation.valid ? 'ok' : 'bad') : undefined) : undefined,
          },
          { label: 'YAML lines', value: yaml.split('\n').length },
        ]}
      />

      {/* Status banners */}
      {error && (
        <div className="mb-4 p-3 rounded-lg bg-red-500/10 border border-red-500/30 flex items-center gap-2 text-red-400 text-sm">
          <XCircle className="w-4 h-4 shrink-0" /> {error}
        </div>
      )}
      {success && (
        <div className="mb-4 p-3 rounded-lg bg-green-500/10 border border-green-500/30 flex items-center gap-2 text-green-400 text-sm">
          <CheckCircle className="w-4 h-4 shrink-0" /> {success}
        </div>
      )}

      {/* Main grid */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* ===== LEFT: Form ===== */}
        <div className="lg:col-span-2 space-y-5">
          {/* Basic Info */}
          <div className={sectionCls}>
            <h2 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
              <Shield className="w-4 h-4 text-purple-400" /> Basic Info
            </h2>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <div>
                <label className="block text-xs font-medium text-slate-400 mb-1.5">
                  Policy Name <span className="text-red-400">*</span>
                </label>
                <input
                  className={inputCls}
                  placeholder="my-policy"
                  value={form.name}
                  onChange={(e) => updateField('name', e.target.value)}
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-slate-400 mb-1.5">
                  Namespace <span className="text-red-400">*</span>
                </label>
                <input
                  className={inputCls}
                  placeholder="default"
                  value={form.namespace}
                  onChange={(e) => updateField('namespace', e.target.value)}
                />
              </div>
              <div className="sm:col-span-2">
                <label className="block text-xs font-medium text-slate-400 mb-1.5">
                  Description <span className="text-slate-600">(optional)</span>
                </label>
                <input
                  className={inputCls}
                  placeholder="What does this policy do?"
                  value={form.description}
                  onChange={(e) => updateField('description', e.target.value)}
                />
              </div>
            </div>
          </div>

          {/* Endpoint Selector */}
          <div className={sectionCls}>
            <h2 className="text-sm font-semibold text-white mb-1 flex items-center gap-2">
              <ArrowRight className="w-4 h-4 text-cyan-400" /> Endpoint Selector
            </h2>
            <p className="text-xs text-slate-500 mb-4">
              Match labels for the pods this policy applies to
            </p>
            <LabelList
              labels={form.endpointLabels}
              onChange={(labels) => updateField('endpointLabels', labels)}
            />
          </div>

          {/* Ingress Rules */}
          <div className={sectionCls}>
            <div className="flex items-center justify-between mb-4">
              <div>
                <h2 className="text-sm font-semibold text-white flex items-center gap-2">
                  <ArrowRight className="w-4 h-4 text-green-400 rotate-180" /> Ingress Rules
                </h2>
                <p className="text-xs text-slate-500 mt-0.5">
                  Define allowed inbound traffic sources
                </p>
              </div>
              <button
                onClick={() =>
                  updateField('ingress', [...form.ingress, defaultIngressRule()])
                }
                className={`${btnSmCls} text-green-400 hover:text-green-300 hover:bg-green-500/10 border border-slate-700/50`}
              >
                <Plus className="w-3.5 h-3.5" /> Add Ingress Rule
              </button>
            </div>

            {form.ingress.length === 0 && (
              <p className="text-xs text-slate-500 italic">
                No ingress rules — all inbound traffic will be denied by default.
              </p>
            )}

            <div className="space-y-4">
              {form.ingress.map((rule, ri) => (
                <div
                  key={ri}
                  className="rounded-lg border border-slate-700/40 bg-slate-900/40 p-4 space-y-4"
                >
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-semibold text-slate-300">
                      Rule {ri + 1}
                    </span>
                    <button
                      onClick={() =>
                        updateField(
                          'ingress',
                          form.ingress.filter((_, j) => j !== ri),
                        )
                      }
                      className="text-slate-500 hover:text-red-400 transition-colors p-1"
                      title="Remove rule"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>

                  {/* Source type */}
                  <div>
                    <label className="text-xs font-medium text-slate-400 mb-2 block">
                      Source Type
                    </label>
                    <div className="flex items-center gap-4">
                      {ingressSourceTypes.map((t) => (
                        <label key={t} className="flex items-center gap-1.5 text-sm text-slate-300 cursor-pointer">
                          <input
                            type="radio"
                            name={`ingress-source-${ri}`}
                            className={radioCls}
                            checked={rule.sourceType === t}
                            onChange={() => updateIngress(ri, { sourceType: t })}
                          />
                          {t}
                        </label>
                      ))}
                    </div>
                  </div>

                  {/* Source details */}
                  {rule.sourceType === 'Labels' && (
                    <div>
                      <label className="text-xs font-medium text-slate-400 mb-2 block">
                        Source Labels
                      </label>
                      <LabelList
                        labels={rule.labels}
                        onChange={(labels) => updateIngress(ri, { labels })}
                      />
                    </div>
                  )}
                  {rule.sourceType === 'CIDR' && (
                    <div>
                      <label className="text-xs font-medium text-slate-400 mb-1.5 block">
                        Source CIDR
                      </label>
                      <input
                        className={inputCls}
                        placeholder="10.0.0.0/8"
                        value={rule.cidr}
                        onChange={(e) => updateIngress(ri, { cidr: e.target.value })}
                      />
                    </div>
                  )}
                  {rule.sourceType === 'Any' && (
                    <p className="text-xs text-slate-500 italic">
                      Matches all sources (allow any ingress).
                    </p>
                  )}

                  {/* Ports */}
                  <PortList
                    ports={rule.ports}
                    onChange={(ports) => updateIngress(ri, { ports })}
                  />
                </div>
              ))}
            </div>
          </div>

          {/* Egress Rules */}
          <div className={sectionCls}>
            <div className="flex items-center justify-between mb-4">
              <div>
                <h2 className="text-sm font-semibold text-white flex items-center gap-2">
                  <ArrowRight className="w-4 h-4 text-orange-400" /> Egress Rules
                </h2>
                <p className="text-xs text-slate-500 mt-0.5">
                  Define allowed outbound traffic destinations
                </p>
              </div>
              <button
                onClick={() =>
                  updateField('egress', [...form.egress, defaultEgressRule()])
                }
                className={`${btnSmCls} text-orange-400 hover:text-orange-300 hover:bg-orange-500/10 border border-slate-700/50`}
              >
                <Plus className="w-3.5 h-3.5" /> Add Egress Rule
              </button>
            </div>

            {form.egress.length === 0 && (
              <p className="text-xs text-slate-500 italic">
                No egress rules — outbound traffic is unrestricted unless other policies apply.
              </p>
            )}

            <div className="space-y-4">
              {form.egress.map((rule, ri) => (
                <div
                  key={ri}
                  className="rounded-lg border border-slate-700/40 bg-slate-900/40 p-4 space-y-4"
                >
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-semibold text-slate-300">
                      Rule {ri + 1}
                    </span>
                    <button
                      onClick={() =>
                        updateField(
                          'egress',
                          form.egress.filter((_, j) => j !== ri),
                        )
                      }
                      className="text-slate-500 hover:text-red-400 transition-colors p-1"
                      title="Remove rule"
                    >
                      <Trash2 className="w-4 h-4" />
                    </button>
                  </div>

                  {/* Destination type */}
                  <div>
                    <label className="text-xs font-medium text-slate-400 mb-2 block">
                      Destination Type
                    </label>
                    <div className="flex flex-wrap items-center gap-4">
                      {egressDestTypes.map((t) => (
                        <label key={t} className="flex items-center gap-1.5 text-sm text-slate-300 cursor-pointer">
                          <input
                            type="radio"
                            name={`egress-dest-${ri}`}
                            className={radioCls}
                            checked={rule.destType === t}
                            onChange={() => updateEgress(ri, { destType: t })}
                          />
                          {t}
                        </label>
                      ))}
                    </div>
                  </div>

                  {/* Destination details */}
                  {rule.destType === 'Labels' && (
                    <div>
                      <label className="text-xs font-medium text-slate-400 mb-2 block">
                        Destination Labels
                      </label>
                      <LabelList
                        labels={rule.labels}
                        onChange={(labels) => updateEgress(ri, { labels })}
                      />
                    </div>
                  )}
                  {rule.destType === 'CIDR' && (
                    <div>
                      <label className="text-xs font-medium text-slate-400 mb-1.5 block">
                        Destination CIDR
                      </label>
                      <input
                        className={inputCls}
                        placeholder="0.0.0.0/0"
                        value={rule.cidr}
                        onChange={(e) => updateEgress(ri, { cidr: e.target.value })}
                      />
                    </div>
                  )}
                  {rule.destType === 'FQDN' && (
                    <div>
                      <label className="text-xs font-medium text-slate-400 mb-1.5 block">
                        Domain Match Pattern
                      </label>
                      <input
                        className={inputCls}
                        placeholder="*.example.com"
                        value={rule.fqdn}
                        onChange={(e) => updateEgress(ri, { fqdn: e.target.value })}
                      />
                      <p className="text-xs text-slate-600 mt-1">
                        Use * for wildcards, e.g. *.googleapis.com
                      </p>
                    </div>
                  )}
                  {rule.destType === 'Service' && (
                    <div className="grid grid-cols-2 gap-3">
                      <div>
                        <label className="text-xs font-medium text-slate-400 mb-1.5 block">
                          Service Name
                        </label>
                        <input
                          className={inputCls}
                          placeholder="my-service"
                          value={rule.serviceName}
                          onChange={(e) =>
                            updateEgress(ri, { serviceName: e.target.value })
                          }
                        />
                      </div>
                      <div>
                        <label className="text-xs font-medium text-slate-400 mb-1.5 block">
                          Service Namespace
                        </label>
                        <input
                          className={inputCls}
                          placeholder="default"
                          value={rule.serviceNamespace}
                          onChange={(e) =>
                            updateEgress(ri, { serviceNamespace: e.target.value })
                          }
                        />
                      </div>
                    </div>
                  )}
                  {rule.destType === 'Any' && (
                    <p className="text-xs text-slate-500 italic">
                      Matches all destinations (allow any egress).
                    </p>
                  )}

                  {/* Ports */}
                  <PortList
                    ports={rule.ports}
                    onChange={(ports) => updateEgress(ri, { ports })}
                  />
                </div>
              ))}
            </div>
          </div>

          {/* Bottom actions */}
          <div className="flex flex-wrap items-center gap-3">
            <button
              onClick={handleReset}
              className={`${btnSmCls} text-slate-400 hover:text-white hover:bg-slate-700/50 border border-slate-700/50`}
            >
              <RotateCcw className="w-3.5 h-3.5" /> Reset
            </button>
            <button
              onClick={handleSimulate}
              disabled={simulating}
              className={`${btnSmCls} text-yellow-400 hover:text-yellow-300 hover:bg-yellow-500/10 border border-slate-700/50 disabled:opacity-50`}
            >
              {simulating ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <FlaskConical className="w-3.5 h-3.5" />
              )}
              Simulate Impact
            </button>
            <button
              onClick={async () => {
                const valid = await handleValidate();
                if (valid) {
                  await handleApply();
                }
              }}
              disabled={applying || validating}
              className={`${btnSmCls} bg-blue-600 text-white hover:bg-blue-500 disabled:opacity-50 shadow-lg shadow-blue-600/20`}
            >
              {applying || validating ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <Play className="w-3.5 h-3.5" />
              )}
              Validate &amp; Apply
            </button>
          </div>
        </div>

        {/* ===== RIGHT: Live YAML Preview ===== */}
        <div className="space-y-4">
          <div className="rounded-xl border border-slate-700/50 bg-slate-800/50 overflow-hidden sticky top-4">
            {/* Preview header */}
            <div className="flex items-center justify-between px-4 py-2.5 border-b border-slate-700/50 bg-slate-900/95">
              <div className="flex items-center gap-2">
                <div className="flex items-center gap-1.5">
                  <span className="w-2.5 h-2.5 rounded-full bg-red-500/80" />
                  <span className="w-2.5 h-2.5 rounded-full bg-yellow-500/80" />
                  <span className="w-2.5 h-2.5 rounded-full bg-green-500/80" />
                </div>
                <h3 className="text-xs font-semibold text-white">Live YAML Preview</h3>
              </div>
              <button
                onClick={handleCopy}
                className={`${btnSmCls} text-slate-400 hover:text-white hover:bg-slate-700/30 border border-slate-700/50`}
              >
                <Copy className="w-3.5 h-3.5" /> Copy
              </button>
            </div>

            {/* YAML code */}
            <pre className="p-4 text-xs text-slate-300 font-mono overflow-auto max-h-[600px] leading-relaxed whitespace-pre">
              <code>{yaml}</code>
            </pre>

            {/* Preview footer with actions */}
            <div className="px-4 py-3 border-t border-slate-700/50 space-y-2">
              <div className="flex items-center gap-2 text-xs text-slate-500">
                <span>{yaml.split('\n').length} lines</span>
                <span className="text-slate-700">|</span>
                <span>{yaml.length} chars</span>
              </div>
              <div className="flex flex-wrap gap-2">
                <button
                  onClick={handleValidate}
                  disabled={validating}
                  className={`${btnSmCls} text-slate-400 hover:text-white hover:bg-slate-700/30 border border-slate-700/50 disabled:opacity-50`}
                >
                  {validating ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <CheckCircle className="w-3.5 h-3.5" />
                  )}
                  Validate
                </button>
                <button
                  onClick={handleApply}
                  disabled={applying}
                  className={`${btnSmCls} bg-blue-600 text-white hover:bg-blue-500 disabled:opacity-50`}
                >
                  {applying ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <Play className="w-3.5 h-3.5" />
                  )}
                  Apply
                </button>
                <button
                  onClick={handleCopy}
                  className={`${btnSmCls} text-slate-400 hover:text-white hover:bg-slate-700/30 border border-slate-700/50`}
                >
                  <Copy className="w-3.5 h-3.5" /> Copy YAML
                </button>
              </div>
            </div>
          </div>

          {/* Validation result */}
          {validation && (
            <div
              className={`rounded-xl border p-4 animate-scale-in ${
                validation.valid
                  ? 'border-green-500/30 bg-green-500/5'
                  : 'border-red-500/30 bg-red-500/5'
              }`}
            >
              <div className="flex items-center gap-2 mb-2">
                {validation.valid ? (
                  <CheckCircle className="w-5 h-5 text-green-400" />
                ) : (
                  <XCircle className="w-5 h-5 text-red-400" />
                )}
                <span
                  className={`font-semibold text-sm ${
                    validation.valid ? 'text-green-400' : 'text-red-400'
                  }`}
                >
                  {validation.valid ? 'Valid Policy' : 'Validation Errors'}
                </span>
              </div>
              {validation.valid && (
                <p className="text-xs text-green-400/80">
                  The generated policy is valid and ready to apply.
                </p>
              )}
              {validation.errors.length > 0 && (
                <ul className="space-y-1 text-xs text-red-400">
                  {validation.errors.map((e, i) => (
                    <li key={i} className="flex items-start gap-1">
                      <span className="mt-0.5">&bull;</span>
                      {e}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}

          {/* Simulation result */}
          {simulationResult && (
            <div className="rounded-xl border border-yellow-500/30 bg-yellow-500/5 p-4">
              <div className="flex items-center gap-2 mb-2">
                <FlaskConical className="w-5 h-5 text-yellow-400" />
                <span className="font-semibold text-sm text-yellow-400">
                  Simulation Result
                </span>
              </div>
              <pre className="text-xs text-yellow-300/80 font-mono whitespace-pre-wrap overflow-auto max-h-60">
                {simulationResult}
              </pre>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default RuleBuilder;
