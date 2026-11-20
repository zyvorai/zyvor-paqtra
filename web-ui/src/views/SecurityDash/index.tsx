// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useCallback, useEffect, useState } from 'react';
import { fetchZeroTrustScore, fetchSecurityFindings } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';
import { scoreTone } from '../../components/kit/tone';
import ScoreGauge from '../../components/ScoreGauge';

export default function SecurityDash() {
  const [score, setScore] = useState<Record<string, number> | null>(null);
  const [findings, setFindings] = useState<{ title?: string; severity?: string; message?: string }[]>([]);
  const [err, setErr] = useState('');

  const load = useCallback(async () => {
    try {
      const [z, f] = await Promise.allSettled([fetchZeroTrustScore(), fetchSecurityFindings()]);
      if (z.status === 'fulfilled') setScore(z.value.data as Record<string, number>);
      if (f.status === 'fulfilled') setFindings(((f.value.data as { findings?: typeof findings }).findings) ?? []);
      const failed = [z, f].find((r) => r.status === 'rejected') as PromiseRejectedResult | undefined;
      setErr(failed ? String(failed.reason) : '');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const tick = useChanged(score);
  return (
    <Board>
      <PagePulse
        headline={tick ? `Security posture ${score?.overall ?? '—'}/100.` : undefined}
        tone={tick ? (typeof score?.overall === 'number' ? scoreTone(score.overall) : undefined) : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'overall', value: tick ? score?.overall ?? '—' : undefined, tone: tick ? ((typeof score?.overall === 'number' ? scoreTone(score.overall) : undefined)) : undefined },
          { label: 'segmentation', value: tick ? score?.network_segmentation ?? '—' : undefined, tone: tick ? ((typeof score?.network_segmentation === 'number' ? scoreTone(score.network_segmentation) : undefined)) : undefined },
          { label: 'identity', value: tick ? score?.identity_verification ?? '—' : undefined, tone: tick ? ((typeof score?.identity_verification === 'number' ? scoreTone(score.identity_verification) : undefined)) : undefined },
          { label: 'encryption', value: tick ? score?.encryption ?? '—' : undefined, tone: tick ? ((typeof score?.encryption === 'number' ? scoreTone(score.encryption) : undefined)) : undefined },
          { label: 'least privilege', value: tick ? score?.least_privilege ?? '—' : undefined, tone: tick ? ((typeof score?.least_privilege === 'number' ? scoreTone(score.least_privilege) : undefined)) : undefined },
          { label: 'monitoring', value: tick ? score?.monitoring ?? '—' : undefined, tone: tick ? ((typeof score?.monitoring === 'number' ? scoreTone(score.monitoring) : undefined)) : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={2}>
        <Eyebrow>SECURITY POSTURE</Eyebrow>
        <h3>Zero-trust board</h3>
        <Toolbar>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
      <Card>
        <Eyebrow>OVERALL SCORE</Eyebrow>
        <div style={{ display: 'flex', justifyContent: 'center', padding: '8px 0' }}>
          <ScoreGauge score={typeof score?.overall === 'number' ? score.overall : 0} label="/ 100" />
        </div>
      </Card>
      <Card span={3}>
        <Eyebrow>FINDINGS</Eyebrow>
        {findings.length === 0 ? <Empty>No security findings.</Empty> : null}
        <div className="list">
          {findings.slice(0, 40).map((f, i) => (
            <div className="agent wide" key={i}>
              <b>{f.title || f.message || `finding-${i}`}</b>
              <span className={`severity-badge ${(f.severity || 'info').toLowerCase()}`}>{f.severity || '—'}</span>
              <small>{f.message && f.title ? f.message : ''}</small>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
