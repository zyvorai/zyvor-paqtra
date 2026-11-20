// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { digestLabel, digestTooltip, loadDigest, type Digest } from './digest';

export default function DigestChip({ onOpen }: { onOpen?: () => void }) {
  const navigate = useNavigate();
  const [digest, setDigest] = useState<Digest | null>(null);

  useEffect(() => {
    let alive = true;
    const load = () => {
      loadDigest().then((d) => {
        if (alive) setDigest(d);
      });
    };
    load();
    const t = setInterval(load, 30000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  if (!digest) return null;
  const sev = (digest.severity || 'info').toLowerCase();
  return (
    <button
      type="button"
      className={`digest-chip ${sev}${digest.changed ? ' changed' : ''}`}
      onClick={() => (onOpen ? onOpen() : navigate('/'))}
      title={digestTooltip(digest)}
      aria-label={`Incident digest ${digestLabel(digest)}`}
    >
      {digestLabel(digest)}
    </button>
  );
}
