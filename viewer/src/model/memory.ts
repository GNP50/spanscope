/** Heap-balance aggregates over the optional `memory` sections of chains and roots. */
import { asBigInt, asNumber, byBigIntDesc } from './format';
import type { ExactChain, ExactProfile, ExactRoot, ProfileIndex } from './types';

export type ExactChainMemory = NonNullable<ExactChain['memory']>;
export type ExactRootMemory = NonNullable<ExactRoot['memory']>;

/** Whether an installed tracking allocator produced memory sections. */
export const hasHeapData = (profile: ExactProfile): boolean => Boolean(profile.meta.capture.heap_tracked);

export interface MemoryRoutine {
  spanId: number;
  calls: bigint;
  allocs: bigint;
  frees: bigint;
  /** Allocated while this routine was the innermost span. */
  allocBytes: bigint;
  freedBytes: bigint;
  inclAllocBytes: bigint;
  inclFreedBytes: bigint;
  /** Inclusive allocated minus freed, summed over calls; positive means retained. */
  netBytes: bigint;
  peakMax: bigint;
  peakSum: bigint;
  /** Largest chain-level 99th percentile peak. */
  peakP99: bigint;
  netMin: bigint;
  netMax: bigint;
  retainingCalls: bigint;
  chains: number[];
}

const signed = (value: number | bigint): bigint => (typeof value === 'bigint' ? value : BigInt(Math.trunc(value)));

/** Per-routine heap balance, largest peak first. Recursive paths are summed like time. */
export function memoryRoutines(index: ProfileIndex, chains: ExactChain[] = index.profile.chains): MemoryRoutine[] {
  const rows = new Map<number, MemoryRoutine>();
  for (const chain of chains) {
    const memory = chain.memory;
    if (!memory) continue;
    const spanId = asNumber(chain.path[chain.path.length - 1]);
    let row = rows.get(spanId);
    if (!row) {
      row = {
        spanId,
        calls: 0n,
        allocs: 0n,
        frees: 0n,
        allocBytes: 0n,
        freedBytes: 0n,
        inclAllocBytes: 0n,
        inclFreedBytes: 0n,
        netBytes: 0n,
        peakMax: 0n,
        peakSum: 0n,
        peakP99: 0n,
        netMin: signed(memory.net_bytes_min),
        netMax: signed(memory.net_bytes_max),
        retainingCalls: 0n,
        chains: [],
      };
      rows.set(spanId, row);
    }
    row.calls += asBigInt(chain.count);
    row.allocs += asBigInt(chain.allocs);
    row.frees += asBigInt(memory.frees);
    row.allocBytes += asBigInt(chain.alloc_bytes);
    row.freedBytes += asBigInt(memory.freed_bytes);
    row.inclAllocBytes += asBigInt(memory.incl_alloc_bytes);
    row.inclFreedBytes += asBigInt(memory.incl_freed_bytes);
    row.netBytes = row.inclAllocBytes - row.inclFreedBytes;
    const peak = asBigInt(memory.peak_bytes_max);
    if (peak > row.peakMax) row.peakMax = peak;
    row.peakSum += asBigInt(memory.peak_bytes_sum);
    const p99 = asBigInt(memory.peak_p99_bytes);
    if (p99 > row.peakP99) row.peakP99 = p99;
    const low = signed(memory.net_bytes_min);
    const high = signed(memory.net_bytes_max);
    if (low < row.netMin) row.netMin = low;
    if (high > row.netMax) row.netMax = high;
    row.retainingCalls += asBigInt(memory.retaining_calls);
    row.chains.push(asNumber(chain.id));
  }
  return [...rows.values()].sort((a, b) => byBigIntDesc(a.peakMax, b.peakMax) || a.spanId - b.spanId);
}

export interface MemoryTotals {
  allocBytes: bigint;
  freedBytes: bigint;
  netBytes: bigint;
  allocs: bigint;
  frees: bigint;
  peakMax: bigint;
  retainingRoutines: number;
}

/** Whole-capture totals from top-level chains, so nested spans are not counted twice. */
export function memoryTotals(index: ProfileIndex, chains: ExactChain[] = index.profile.chains): MemoryTotals {
  const totals: MemoryTotals = { allocBytes: 0n, freedBytes: 0n, netBytes: 0n, allocs: 0n, frees: 0n, peakMax: 0n, retainingRoutines: 0 };
  for (const chain of chains) {
    const memory = chain.memory;
    if (!memory) continue;
    totals.allocs += asBigInt(chain.allocs);
    totals.frees += asBigInt(memory.frees);
    const peak = asBigInt(memory.peak_bytes_max);
    if (peak > totals.peakMax) totals.peakMax = peak;
    if (chain.path.length !== 1) continue;
    totals.allocBytes += asBigInt(memory.incl_alloc_bytes);
    totals.freedBytes += asBigInt(memory.incl_freed_bytes);
  }
  totals.netBytes = totals.allocBytes - totals.freedBytes;
  totals.retainingRoutines = memoryRoutines(index, chains).filter(row => row.netBytes > 0n && row.retainingCalls * 2n >= row.calls).length;
  return totals;
}

export interface RootMemoryPoint {
  uid: string;
  spanId: number;
  startNs: bigint;
  allocBytes: bigint;
  freedBytes: bigint;
  netBytes: bigint;
  peakBytes: bigint;
  /** Running sum of net bytes over the retained roots in start order. */
  cumulativeNetBytes: bigint;
}

/** Heap balance of each retained root in start order; a rising cumulative net suggests growth. */
export function rootMemorySeries(index: ProfileIndex): RootMemoryPoint[] {
  let cumulative = 0n;
  return index.profile.roots
    .filter(root => root.memory)
    .map(root => {
      const memory = root.memory!;
      const alloc = asBigInt(memory.alloc_bytes);
      const freed = asBigInt(memory.freed_bytes);
      cumulative += alloc - freed;
      return {
        uid: root.uid,
        spanId: asNumber(root.span),
        startNs: asBigInt(root.t_start_ns),
        allocBytes: alloc,
        freedBytes: freed,
        netBytes: alloc - freed,
        peakBytes: asBigInt(memory.peak_bytes),
        cumulativeNetBytes: cumulative,
      };
    });
}

/** Merged per-call peak histogram of one routine across its chains. */
export function peakHistogram(index: ProfileIndex, spanId: number): { upperBytes: bigint; count: bigint }[] {
  const buckets = new Map<bigint, bigint>();
  for (const chain of index.profile.chains) {
    if (!chain.memory || asNumber(chain.path[chain.path.length - 1]) !== spanId) continue;
    for (const bucket of chain.memory.peak_histogram) {
      const upper = asBigInt(bucket.upper_bytes);
      buckets.set(upper, (buckets.get(upper) ?? 0n) + asBigInt(bucket.count));
    }
  }
  return [...buckets.entries()].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0)).map(([upperBytes, count]) => ({ upperBytes, count }));
}

export interface MemoryTreeNode {
  name: string;
  spanId: number;
  chainId: number | null;
  /** Inclusive allocated bytes of this call path. */
  value: number;
  children: MemoryTreeNode[];
}

/** Call-path hierarchy weighted by inclusive allocated bytes, for a sunburst. */
export function memoryTree(chains: ExactChain[], name: (spanId: number) => string): MemoryTreeNode[] {
  const byKey = new Map<string, MemoryTreeNode>();
  const roots: MemoryTreeNode[] = [];
  const sorted = [...chains].filter(chain => chain.memory).sort((a, b) => a.path.length - b.path.length);
  for (const chain of sorted) {
    const path = chain.path.map(asNumber);
    const key = path.join('/');
    const node: MemoryTreeNode = {
      name: name(path[path.length - 1]),
      spanId: path[path.length - 1],
      chainId: asNumber(chain.id),
      value: asNumber(chain.memory!.incl_alloc_bytes),
      children: [],
    };
    byKey.set(key, node);
    const parent = byKey.get(path.slice(0, -1).join('/'));
    if (parent) parent.children.push(node);
    else roots.push(node);
  }
  // Sunburst children must not exceed their parent; recursion can sum past it.
  const clamp = (node: MemoryTreeNode) => {
    node.children.forEach(clamp);
    const sum = node.children.reduce((total, child) => total + child.value, 0);
    if (sum > node.value) node.value = sum;
  };
  roots.forEach(clamp);
  return roots.filter(node => node.value > 0);
}
