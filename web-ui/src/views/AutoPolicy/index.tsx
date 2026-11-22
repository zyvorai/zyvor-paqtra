// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useState } from 'react';
import { generateAutopolicy } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

export default function AutoPolicy() {
  const [ns, setNs] = useState('default');
  const [draft, setDraft] = useState('');
  const [err, setErr] = useState('');
  const [busy, setBusy] = useState(false);

  const generate = useCallback(async () => {
    setBusy(true);
    setErr('');
    try {
      const { data } = await generateAutopolicy({ namespace: ns });
      setDraft(typeof data === 'string' ? data : JSON.stringify(data, null, 2));
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setDraft('');
    } finally {
      setBusy(false);
    }
  }, [ns]);

  const tick = useChanged(draft);
  return (
    <Board>
      <PagePulse
        headline={tick ? `Draft for ${ns} ready — review before apply.` : undefined}
        tick={tick}
        live={false}
        error={err || undefined}
        figures={[
          { label: 'draft lines', value: tick ? draft.split("\n").length : undefined },
          { label: 'namespace', value: tick ? ns : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>AUTOPOLICY</Eyebrow>
        <h3>Learn, then review</h3>
        <p>Generate Cilium policies from observed traffic — review before apply. Nothing on this page auto-enforces.</p>
        <Toolbar>
          <label>
            Namespace
            <input value={ns} onChange={(e) => setNs(e.target.value)} />
          </label>
          <button type="button" className="primary" disabled={busy} onClick={() => void generate()}>
            {busy ? 'Generating…' : 'Generate draft'}
          </button>
        </Toolbar>
      </Card>
      <Card span={3}>
        <Eyebrow>DRAFT</Eyebrow>
        {!draft ? <Empty>No draft yet — generate from observed flows.</Empty> : null}
        {draft ? <pre className="terminalbody" style={{ whiteSpace: 'pre-wrap', margin: 0 }}>{draft}</pre> : null}
      </Card>
    </Board>
  );
}
