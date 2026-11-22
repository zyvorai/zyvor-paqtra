// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import { fetchClusterHealth, fetchEbpfDrops } from '../services/api';

export type Digest = {
  severity: string;
  fingerprint: string;
  changed: boolean;
  headline: string;
  whyChanged: string[];
};

export function digestTooltip(d: Digest): string {
  if (!d.changed) return d.headline || 'On-call digest';
  const why = (d.whyChanged || []).join('; ');
  return why ? `${d.headline} — ${why}` : d.headline;
}

export function digestLabel(d: Digest): string {
  const sev = (d.severity || 'info').toLowerCase();
  const fp = d.fingerprint ? d.fingerprint.slice(0, 6) : '';
  if (d.changed) return `${sev} · changed ${fp}`.trim();
  return fp ? `${sev} · ${fp}` : sev;
}

export function buildDigest(opts: {
  score?: number;
  status?: string;
  drops?: number;
  agentsOk?: boolean;
}): Digest {
  const score = opts.score ?? 100;
  const drops = opts.drops ?? 0;
  const status = (opts.status || 'healthy').toLowerCase();
  const why: string[] = [];
  let severity = 'info';
  if (status.includes('unhealthy') || score < 50) {
    severity = 'critical';
    why.push(`cluster ${status}`);
  } else if (status.includes('degraded') || score < 80) {
    severity = 'warning';
    why.push(`health score ${score}`);
  }
  if (drops > 1000) {
    severity = severity === 'info' ? 'warning' : severity;
    why.push(`${drops.toLocaleString()} packets dropped by Cilium`);
  }
  const fingerprint = shortHash(`${score}|${drops}|${status}|${severity}`);
  return {
    severity,
    fingerprint,
    changed: why.length > 0,
    headline: why.length ? why[0] : `Cluster ${status} · score ${score}`,
    whyChanged: why,
  };
}

function shortHash(s: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) h = Math.imul(h ^ s.charCodeAt(i), 0x01000193);
  return (h >>> 0).toString(16).padStart(8, '0').slice(0, 6);
}

/** Packets dropped across every Cilium drop reason (`total_drops` counts reason entries). */
export function dropPackets(data: { drops?: { count?: number }[]; total_drops?: number } | null | undefined): number {
  if (!data) return 0;
  if (data.drops?.length) return data.drops.reduce((n, d) => n + (d.count ?? 0), 0);
  return data.total_drops ?? 0;
}

/** Client-side digest from health + drops (until a dedicated digest API exists). */
export async function loadDigest(): Promise<Digest | null> {
  try {
    const [h, d] = await Promise.allSettled([fetchClusterHealth(), fetchEbpfDrops()]);
    const health = h.status === 'fulfilled' ? h.value.data : null;
    const drops = d.status === 'fulfilled' ? dropPackets(d.value.data) : 0;
    return buildDigest({
      score: health?.score ?? health?.health_score,
      status: health?.overall ?? health?.status,
      drops,
    });
  } catch {
    return null;
  }
}
