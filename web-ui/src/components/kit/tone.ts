// Color is deviation (deviation-only color): 'ok' is confirmed good,
// 'warn'/'bad' only when a value really deviates, 'idle' when there is no data yet.
export type Tone = 'ok' | 'warn' | 'bad' | 'idle';

/** Tone for a count where any nonzero value is a deviation. */
export function countTone(n: number | undefined, bad = Infinity): Tone {
  if (n === undefined || !Number.isFinite(n)) return 'idle';
  if (n >= bad) return 'bad';
  return n > 0 ? 'warn' : 'ok';
}

/** Tone for a rate (e.g. an error fraction); idle when it is not a number. */
export function rateTone(rate: unknown): Tone {
  return typeof rate === 'number' ? countTone(rate) : 'idle';
}

/** Tone for a 0–100 score where higher is better. */
export function scoreTone(score: number | undefined, warnBelow = 80, badBelow = 50): Tone {
  if (score === undefined || !Number.isFinite(score)) return 'idle';
  if (score < badBelow) return 'bad';
  return score < warnBelow ? 'warn' : 'ok';
}
