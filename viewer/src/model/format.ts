/** Formatting and exact-integer helpers shared by every view. */

export const asNumber = (value: number | bigint): number => (typeof value === 'bigint' ? Number(value) : value);
export const asBigInt = (value: number | bigint): bigint => (typeof value === 'bigint' ? value : BigInt(value));

export const formatInteger = (value: number | bigint): string => asBigInt(value).toLocaleString('en-US');

export function formatNs(value: number | bigint): string {
  const number = asNumber(value);
  if (number >= 1e9) return `${(number / 1e9).toFixed(2)} s`;
  if (number >= 1e6) return `${(number / 1e6).toFixed(2)} ms`;
  if (number >= 1e3) return `${(number / 1e3).toFixed(1)} µs`;
  return `${formatInteger(value)} ns`;
}

export function formatBytes(value: number | bigint): string {
  const number = asNumber(value);
  if (number >= 1024 ** 3) return `${(number / 1024 ** 3).toFixed(2)} GiB`;
  if (number >= 1024 ** 2) return `${(number / 1024 ** 2).toFixed(2)} MiB`;
  if (number >= 1024) return `${(number / 1024).toFixed(1)} KiB`;
  return `${formatInteger(value)} B`;
}

/** Percentage of `part` in `whole`, 0–100 with two decimals of precision. */
export function ratio(part: bigint, whole: bigint): number {
  if (whole === 0n) return 0;
  return Math.min(100, Math.round(Number((part * 10000n) / whole)) / 100);
}

export function formatPercent(part: number | bigint, whole: number | bigint): string {
  const total = asNumber(whole);
  if (total === 0) return '—';
  return `${((asNumber(part) / total) * 100).toFixed(1)}%`;
}

export const exactJson = (value: unknown): string =>
  JSON.stringify(value, (_key, entry: unknown) => (typeof entry === 'bigint' ? entry.toString() : entry), 2);

export const maxBigInt = (values: Iterable<bigint>): bigint => {
  let max = 0n;
  for (const value of values) if (value > max) max = value;
  return max;
};

/** Sort comparator: larger bigint first. */
export const byBigIntDesc = (left: bigint, right: bigint): number => (right > left ? 1 : right < left ? -1 : 0);

/** The last two `::` segments of a qualified name, the one short form used everywhere. */
export const shortName = (name: string): string => name.split('::').slice(-2).join('::');

export const label = (value: string): string => value.replaceAll('_', ' ').replace(/^./, character => character.toUpperCase());
