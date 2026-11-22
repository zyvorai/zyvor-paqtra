// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { useCallback, useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { fetchPolicies, type Policy } from '../../services/api';
import { Board, Card, Eyebrow, Warning, Empty, Toolbar } from '../../components/Board';
import PagePulse from '../../components/kit/PagePulse';
import { useChanged } from '../../components/kit/useSeries';

export default function Policies() {
  const [policies, setPolicies] = useState<Policy[]>([]);
  const [err, setErr] = useState('');
  const [ns, setNs] = useState('');

  const load = useCallback(async () => {
    try {
      setPolicies((await fetchPolicies()).data.policies ?? []);
      setErr('');
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const filtered = ns ? policies.filter((p) => p.namespace === ns) : policies;
  const active = policies.filter((p) => (p.status || '').toLowerCase().includes('active') || (p.status || '').toLowerCase().includes('enforc')).length;

  const tick = useChanged(policies);
  return (
    <Board>
      <PagePulse
        headline={tick ? `${policies.length} Cilium policies across ${new Set(policies.map((p) => p.namespace)).size} namespaces.` : undefined}
        tick={tick}
        error={err || undefined}
        figures={[
          { label: 'policies', value: tick ? policies.length : undefined },
          { label: 'active/enforcing', value: tick ? active : undefined },
          { label: 'namespaces', value: tick ? new Set(policies.map((p) => p.namespace)).size : undefined },
        ]}
      />
      {err ? (
        <Card span={3}>
          <Warning>{err}</Warning>
        </Card>
      ) : null}
      <Card span={3}>
        <Eyebrow>POLICIES</Eyebrow>
        <h3>Cilium workbench</h3>
        <p>Plan and apply CiliumNetworkPolicy when CRDs are present. Paqtra does not write Cilium BPF maps.</p>
        <Toolbar>
          <label>
            Namespace
            <input value={ns} placeholder="all" onChange={(e) => setNs(e.target.value)} />
          </label>
          <button type="button" className="btn-refresh" onClick={() => void load()}>
            Refresh
          </button>
        </Toolbar>
      </Card>
      <Card span={3}>
        <Eyebrow>INVENTORY</Eyebrow>
        {filtered.length === 0 ? <Empty>No policies found.</Empty> : null}
        <div className="list">
          {filtered.slice(0, 100).map((p) => (
            <div className="agent wide" key={p.id || `${p.namespace}/${p.name}`}>
              <b>
                {p.namespace}/{p.name}
              </b>
              <span>{p.status || '—'}</span>
              <small>{p.created_at ? new Date(p.created_at).toLocaleString() : '—'}</small>
              <Link to={`/policy-rules?policy=${encodeURIComponent(p.id || `${p.namespace}/${p.name}`)}`}>Edit rules</Link>
            </div>
          ))}
        </div>
      </Card>
    </Board>
  );
}
