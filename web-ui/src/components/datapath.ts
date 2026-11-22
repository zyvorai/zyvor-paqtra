// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// "kube-system/hubble-relay-59cc868d4d-jmg7l" -> "hubble-relay"
export function workloadLabel(name: string, max = 18): string {
  const pod = name.split(' (')[0].split('/').pop() || name;
  const deployment = /-[a-z0-9]{8,10}-[a-z0-9]{5}$/;
  const short = deployment.test(pod) ? pod.replace(deployment, '') : pod.replace(/-[a-z0-9]{5}$/, '');
  return short.length > max ? short.slice(0, max - 1) + '…' : short;
}

export function flowDuration(perSecond?: number): number {
  if (!perSecond || perSecond <= 0) return 6;
  return Math.max(1.2, 6 - Math.log10(perSecond + 1) * 1.1);
}
