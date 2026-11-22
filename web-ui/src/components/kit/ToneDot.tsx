// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

import type { Tone } from './tone';

export default function ToneDot({ tone, label }: { tone: Tone; label?: string }) {
  return <span className={`kit-dot tone-${tone}`} role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : true} />;
}
