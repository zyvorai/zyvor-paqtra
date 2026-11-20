// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useState } from 'react';
import {
  fetchFrameworks, fetchAudits, runAudit, fetchAuditReport, apiErrorMessage,
  ComplianceFramework, AuditSummary, AuditResult, ReportFormat,
} from '../../services/api';
import { openBlobInNewTab, downloadBlob, blobErrorMessage } from '../../services/reportFiles';
import { useAuthStore } from '../../stores/authStore';
import { useAutoRefresh } from '../../hooks/useAutoRefresh';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

const STATUS_LABEL: Record<string, string> = { passed: 'PASSED', failed: 'FAILED', skipped: 'NOT EVALUATED' };

const label = (f: ComplianceFramework | undefined, id: string): string => (f ? `${f.name} ${f.version}` : id);

export default function Compliance() {
  const role = useAuthStore((s) => s.role);
  const [frameworks, setFrameworks] = useState<ComplianceFramework[]>([]);
  const [audits, setAudits] = useState<AuditSummary[]>([]);
  const [selected, setSelected] = useState('');
  const [latest, setLatest] = useState<AuditResult | null>(null);
  const [running, setRunning] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      const [fw, au] = await Promise.all([fetchFrameworks(), fetchAudits()]);
      setFrameworks(fw.data.frameworks ?? []);
      setAudits(au.data.audits ?? []);
      setSelected((cur) => cur || fw.data.frameworks?.[0]?.id || '');
      setErr('');
    } catch (e) {
      setErr(apiErrorMessage(e, 'Failed to load compliance data'));
    }
  }, []);

  // Fetches on mount; no polling, so a result on screen is never replaced under the reader.
  const { manualRefresh } = useAutoRefresh(load, 30000, false);

  const byId = (id: string) => frameworks.find((f) => f.id === id);
  // Only a viewer is known to be refused; the server enforces the rest.
  const canRun = role !== 'viewer';

  const handleRun = async () => {
    setRunning(true);
    setErr('');
    try {
      const { data } = await runAudit(selected);
      setLatest(data);
      await load();
    } catch (e) {
      setErr(apiErrorMessage(e, 'Failed to run the checks'));
    } finally {
      setRunning(false);
    }
  };

  const handleReport = async (a: AuditSummary, format: ReportFormat) => {
    setBusy(`${a.audit_id}:${format}`);
    setErr('');
    try {
      const { data } = await fetchAuditReport(a.audit_id, format);
      if (format === 'html') openBlobInNewTab(data);
      else downloadBlob(data, `paqtra-checks-${a.framework}-${a.audit_id.slice(0, 8)}.${format}`);
    } catch (e) {
      setErr(await blobErrorMessage(e, 'Failed to export the report'));
    } finally {
      setBusy(null);
    }
  };

  const tick = useChanged(frameworks);
  return (
    <Board>
      <PagePulse
        headline={tick ? `${frameworks.length} frameworks, ${audits.length} stored runs — network-layer checks, not an assessment.` : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'frameworks', value: tick ? frameworks.length : undefined },
          { label: 'stored runs', value: tick ? audits.length : undefined },
        ]}
      />
      {err ? <Card span={3}><Warning>{err}</Warning></Card> : null}

      <Card span={3}>
        <Eyebrow>COMPLIANCE</Eyebrow>
        <h3>Network security checks</h3>
        <p>
          These are Paqtra&apos;s own automated network-layer checks: policy coverage, flow monitoring, transparent encryption and
          dropped flows. <b>They are not a compliance assessment.</b> They are not mapped to individual controls of the framework
          a run is filed under, so passing them does not show compliance with it. A check that could not read its input is reported as
          not evaluated, never as passed.
        </p>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={manualRefresh}>Refresh</button>
        </Toolbar>
      </Card>

      {canRun ? (
        <Card span={3}>
          <Eyebrow>RUN</Eyebrow>
          <Toolbar>
            <label>
              File under
              <select value={selected} onChange={(e) => setSelected(e.target.value)} disabled={running}>
                {frameworks.map((f) => <option key={f.id} value={f.id}>{label(f, f.id)}</option>)}
              </select>
            </label>
            <button type="button" className="primary" disabled={running || !selected} onClick={handleRun}>
              {running ? 'Running…' : 'Run network checks'}
            </button>
          </Toolbar>
          {byId(selected) ? (
            <p>
              {byId(selected)!.name} {byId(selected)!.version} defines {byId(selected)!.control_count} controls; the checks below are
              not mapped to them. The framework is recorded as context.
            </p>
          ) : null}
        </Card>
      ) : null}

      {latest ? (
        <Card span={3}>
          <Eyebrow>LATEST RUN</Eyebrow>
          <h3>{latest.passed} of {latest.total_controls} checks passed</h3>
          <p>
            {latest.failed} failed, {latest.skipped} not evaluated
            {latest.passed + latest.failed > 0 ? `; ${latest.score}% of the evaluated checks passed` : ''}.
            {' '}Dropped flows are judged on the {latest.flows_sampled} most recent flows, not a period of time.
          </p>
          <div className="list">
            {latest.findings.map((f) => (
              <div className="agent wide" key={f.control_id}>
                <b>{f.title}</b>
                <span className={`severity-badge ${f.status === 'passed' ? 'info' : f.status === 'failed' ? f.severity : 'warning'}`}>
                  {STATUS_LABEL[f.status] ?? f.status.toUpperCase()}
                </span>
                <small>{f.control_id} · {f.severity} · {f.description}</small>
              </div>
            ))}
          </div>
        </Card>
      ) : null}

      <Card span={3}>
        <Eyebrow>HISTORY</Eyebrow>
        {audits.length === 0 ? <Empty>No runs stored yet.{canRun ? ' Run the checks above to create one.' : ''}</Empty> : null}
        <div className="list">
          {audits.map((a) => (
            <div className="agent wide" key={a.audit_id}>
              <b>{label(byId(a.framework), a.framework)}</b>
              <span>{a.passed} of {a.total_controls} passed</span>
              <small>
                {a.failed} failed · {a.skipped} not evaluated ·{' '}
                {a.completed_at ? new Date(a.completed_at).toLocaleString() : 'not completed'}
                {a.requested_by ? ` · by ${a.requested_by}` : ''}
              </small>
              {(['html', 'csv', 'json'] as ReportFormat[]).map((fmt) => (
                <button
                  key={fmt}
                  type="button"
                  className="btn-refresh"
                  disabled={busy !== null}
                  onClick={() => void handleReport(a, fmt)}
                  aria-label={`${fmt === 'html' ? 'Open report' : `Download ${fmt.toUpperCase()}`} for ${a.audit_id.slice(0, 8)}`}
                >
                  {fmt === 'html' ? 'Report' : fmt.toUpperCase()}
                </button>
              ))}
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
