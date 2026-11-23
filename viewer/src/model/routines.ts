import { asBigInt, asNumber, byBigIntDesc } from './format';
import type { ExactChain, ProfileIndex } from './types';

export interface CallRelation {
  spanId: number;
  calls: bigint;
  inclusiveWallNs: bigint;
}

export interface RoutineSummary {
  spanId: number;
  calls: bigint;
  inclusiveWallNs: bigint;
  selfActiveNs: bigint;
  allocBytes: bigint;
  callers: CallRelation[];
  callees: CallRelation[];
  paths: { chain: ExactChain; calls: bigint; inclusiveWallNs: bigint; selfActiveNs: bigint }[];
}

interface MutableRoutine extends Omit<RoutineSummary, 'callers' | 'callees'> {
  callers: Map<number, CallRelation>;
  callees: Map<number, CallRelation>;
}

function addRelation(target: Map<number, CallRelation>, spanId: number, calls: bigint, inclusiveWallNs: bigint) {
  const current = target.get(spanId) ?? { spanId, calls: 0n, inclusiveWallNs: 0n };
  current.calls += calls;
  current.inclusiveWallNs += inclusiveWallNs;
  target.set(spanId, current);
}

/** Aggregate by terminal routine. Root-local values come only from retained root records. */
export function summarizeRoutines(index: ProfileIndex, rootUid: string | null): RoutineSummary[] {
  const root = rootUid ? index.roots.get(rootUid) : null;
  if (rootUid && !root) return [];
  const observed = root
    ? root.chains.map(([id, count, total]) => {
      const chain = index.chains.get(asNumber(id));
      return chain ? { chain, count: asBigInt(count), total: asBigInt(total), self: asBigInt(root.chain_self_ns[String(id)] ?? 0) } : null;
    }).filter((item): item is NonNullable<typeof item> => item !== null)
    : index.profile.chains.map(chain => ({ chain, count: asBigInt(chain.count), total: asBigInt(chain.total_ns), self: asBigInt(chain.self_ns) }));
  const routines = new Map<number, MutableRoutine>();
  const ensure = (id: number): MutableRoutine => {
    let row = routines.get(id);
    if (!row) {
      row = { spanId: id, calls: 0n, inclusiveWallNs: 0n, selfActiveNs: 0n, allocBytes: 0n, callers: new Map(), callees: new Map(), paths: [] };
      routines.set(id, row);
    }
    return row;
  };
  for (const { chain, count, total, self } of observed) {
    const path = chain.path.map(asNumber);
    const leaf = path[path.length - 1];
    const row = ensure(leaf);
    row.calls += count;
    row.inclusiveWallNs += total;
    row.selfActiveNs += self;
    if (!root) row.allocBytes += asBigInt(chain.alloc_bytes);
    row.paths.push({ chain, calls: count, inclusiveWallNs: total, selfActiveNs: self });
    if (path.length > 1) {
      const parent = path[path.length - 2];
      addRelation(row.callers, parent, count, total);
      addRelation(ensure(parent).callees, leaf, count, total);
    }
  }
  const sorted = (relations: Map<number, CallRelation>) =>
    [...relations.values()].sort((a, b) => byBigIntDesc(a.calls, b.calls) || a.spanId - b.spanId);
  return [...routines.values()]
    .map(row => ({
      ...row,
      callers: sorted(row.callers),
      callees: sorted(row.callees),
      paths: row.paths.sort((a, b) => byBigIntDesc(a.calls, b.calls) || asNumber(a.chain.id) - asNumber(b.chain.id)),
    }))
    .sort((a, b) => byBigIntDesc(a.selfActiveNs, b.selfActiveNs) || a.spanId - b.spanId);
}

export function metricNames(index: ProfileIndex): string[] {
  return [...new Set(index.profile.roots.flatMap(root => Object.keys(root.metrics)))].sort();
}

/** Calls and exclusive time of one routine in each retained run that includes it. */
export function routineByRoot(index: ProfileIndex, spanId: number): { uid: string; calls: bigint; selfActiveNs: bigint }[] {
  const rows: { uid: string; calls: bigint; selfActiveNs: bigint }[] = [];
  for (const root of index.profile.roots) {
    let calls = 0n;
    let self = 0n;
    for (const [id, count] of root.chains) {
      const chain = index.chains.get(asNumber(id));
      if (!chain || asNumber(chain.path[chain.path.length - 1]) !== spanId) continue;
      calls += asBigInt(count);
      self += asBigInt(root.chain_self_ns[String(id)] ?? 0);
    }
    if (calls > 0n) rows.push({ uid: root.uid, calls, selfActiveNs: self });
  }
  return rows;
}
