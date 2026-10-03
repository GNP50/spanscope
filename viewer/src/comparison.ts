import { asBigInt, asNumber, type ExactChain, type ExactSpan, type ProfileIndex } from './core';

export interface RoutineMeasurement {
  key: string;
  name: string;
  file: string;
  line: number;
  calls: bigint;
  totalNs: bigint;
  selfNs: bigint;
  p99Ns: bigint;
  allocBytes: bigint;
}

export interface RoutineComparison {
  key: string;
  name: string;
  file: string;
  line: number;
  baseline: RoutineMeasurement | null;
  candidate: RoutineMeasurement | null;
}

export interface ProfileComparison {
  rows: RoutineComparison[];
  baselineSelfNs: bigint;
  candidateSelfNs: bigint;
  baselineCalls: bigint;
  candidateCalls: bigint;
  regressions: number;
  improvements: number;
  added: number;
  removed: number;
  warnings: string[];
}

interface MutableMeasurement extends Omit<RoutineMeasurement, 'p99Ns'> {
  buckets: Map<bigint, bigint>;
}

function routineKey(span: ExactSpan): string {
  // Profile IDs and source lines can change between builds. Fully qualified names plus
  // source files remain useful across ordinary edits while still separating modules.
  return `${span.name}\u0000${span.file}`;
}

function mergeHistogram(target: Map<bigint, bigint>, chain: ExactChain) {
  for (const bucket of chain.histogram.buckets) {
    const upper = asBigInt(bucket.upper_ns);
    target.set(upper, (target.get(upper) ?? 0n) + asBigInt(bucket.count));
  }
}

function percentile(buckets: Map<bigint, bigint>, count: bigint, numerator: bigint): bigint {
  if (count === 0n) return 0n;
  const target = (count * numerator + 99n) / 100n;
  let seen = 0n;
  for (const [upper, bucketCount] of [...buckets].sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0)) {
    seen += bucketCount;
    if (seen >= target) return upper;
  }
  return 0n;
}

export function aggregateRoutines(index: ProfileIndex): Map<string, RoutineMeasurement> {
  const measurements = new Map<string, MutableMeasurement>();
  for (const chain of index.profile.chains) {
    const span = index.spans.get(asNumber(chain.path[chain.path.length - 1]));
    if (!span) continue;
    const key = routineKey(span);
    let row = measurements.get(key);
    if (!row) {
      row = {
        key,
        name: span.name,
        file: span.file,
        line: asNumber(span.line),
        calls: 0n,
        totalNs: 0n,
        selfNs: 0n,
        allocBytes: 0n,
        buckets: new Map(),
      };
      measurements.set(key, row);
    }
    row.line = Math.min(row.line, asNumber(span.line));
    row.calls += asBigInt(chain.count);
    row.totalNs += asBigInt(chain.total_ns);
    row.selfNs += asBigInt(chain.self_ns);
    row.allocBytes += asBigInt(chain.alloc_bytes);
    mergeHistogram(row.buckets, chain);
  }
  return new Map([...measurements].map(([key, row]) => [key, {
    key: row.key,
    name: row.name,
    file: row.file,
    line: row.line,
    calls: row.calls,
    totalNs: row.totalNs,
    selfNs: row.selfNs,
    p99Ns: percentile(row.buckets, row.calls, 99n),
    allocBytes: row.allocBytes,
  }]));
}

export function relativeDelta(baseline: bigint, candidate: bigint): number | null {
  if (baseline === 0n) return candidate === 0n ? 0 : null;
  return Number(candidate) / Number(baseline) - 1;
}

export function compareProfiles(baseline: ProfileIndex, candidate: ProfileIndex): ProfileComparison {
  const before = aggregateRoutines(baseline);
  const after = aggregateRoutines(candidate);
  const rows = [...new Set([...before.keys(), ...after.keys()])].map(key => {
    const left = before.get(key) ?? null;
    const right = after.get(key) ?? null;
    const source = right ?? left!;
    return { key, name: source.name, file: source.file, line: source.line, baseline: left, candidate: right };
  });
  rows.sort((left, right) => {
    const leftDelta = (left.candidate?.selfNs ?? 0n) - (left.baseline?.selfNs ?? 0n);
    const rightDelta = (right.candidate?.selfNs ?? 0n) - (right.baseline?.selfNs ?? 0n);
    return rightDelta > leftDelta ? 1 : rightDelta < leftDelta ? -1 : left.name.localeCompare(right.name);
  });
  let regressions = 0;
  let improvements = 0;
  for (const row of rows) {
    if (!row.baseline || !row.candidate) continue;
    const delta = relativeDelta(row.baseline.selfNs, row.candidate.selfNs);
    if (delta !== null && delta >= 0.05) regressions += 1;
    if (delta !== null && delta <= -0.05) improvements += 1;
  }
  const warnings: string[] = [];
  if (baseline.profile.meta.program !== candidate.profile.meta.program) warnings.push(`Programs differ: ${baseline.profile.meta.program} vs ${candidate.profile.meta.program}.`);
  if (asNumber(baseline.profile.meta.capture.sample_rate) !== asNumber(candidate.profile.meta.capture.sample_rate)) warnings.push('Sampling rates differ; call counts and aggregate costs may not be directly comparable.');
  if (!baseline.profile.meta.capture.snapshot_complete || !candidate.profile.meta.capture.snapshot_complete) warnings.push('At least one snapshot is incomplete. Interpret aggregate deltas with care.');
  return {
    rows,
    baselineSelfNs: baseline.totalSelfNs,
    candidateSelfNs: candidate.totalSelfNs,
    baselineCalls: baseline.totalCalls,
    candidateCalls: candidate.totalCalls,
    regressions,
    improvements,
    added: rows.filter(row => !row.baseline).length,
    removed: rows.filter(row => !row.candidate).length,
    warnings,
  };
}
