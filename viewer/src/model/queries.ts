import { asBigInt, asNumber, shortName } from './format';
import type { ExactChain, ExactInstance, ExactRoot, Filters, ProfileIndex } from './types';

export const leafSpan = (chain: ExactChain): number => asNumber(chain.path[chain.path.length - 1]);

export const spanName = (index: ProfileIndex, spanId: number): string => index.spans.get(spanId)?.name ?? `Span #${spanId}`;
export const spanShortName = (index: ProfileIndex, spanId: number): string => shortName(spanName(index, spanId));

export function chainName(index: ProfileIndex, chainId: number): string {
  const chain = index.chains.get(chainId);
  return chain ? spanName(index, leafSpan(chain)) : `Chain #${chainId}`;
}

export const rootName = (index: ProfileIndex, root: ExactRoot): string => spanName(index, asNumber(root.span));

export function typeName(index: ProfileIndex, typeId: number): string {
  return index.objects?.types.get(typeId)?.name ?? `Type #${typeId}`;
}

export function instanceName(index: ProfileIndex, instance: ExactInstance): string {
  return `${shortName(typeName(index, asNumber(instance.type_id)))} ${instance.key}`;
}

export function scopeLabel(index: ProfileIndex, rootUid: string | null): string {
  const root = rootUid ? index.roots.get(rootUid) : null;
  return root ? `Selected run · ${rootName(index, root)}` : 'Entire captured profile';
}

export function filterChains(index: ProfileIndex, filters: Filters): ExactChain[] {
  const query = filters.query.trim().toLowerCase();
  const allowed = filters.rootUid ? new Set(index.roots.get(filters.rootUid)?.chains.map(entry => asNumber(entry[0])) ?? []) : null;
  return index.profile.chains.filter(chain => {
    if (allowed && !allowed.has(asNumber(chain.id))) return false;
    if (filters.spanId !== null && !chain.path.some(id => asNumber(id) === filters.spanId)) return false;
    if (!query) return true;
    return chain.path.some(id => index.spans.get(asNumber(id))?.name.toLowerCase().includes(query));
  });
}

export interface FlameNode {
  key: string;
  spanId: number;
  depth: number;
  parent: string | null;
  self: bigint;
  weight: bigint;
}

/** What flame widths measure: exclusive active time, or bytes allocated directly. */
export type FlameMetric = 'time' | 'alloc';

export function buildFlame(chains: ExactChain[], metric: FlameMetric = 'time'): FlameNode[] {
  const nodes = new Map<string, FlameNode>();
  for (const chain of chains) {
    let parent: string | null = null;
    chain.path.forEach((id, depth) => {
      const key = chain.path.slice(0, depth + 1).join('/');
      if (!nodes.has(key)) nodes.set(key, { key, spanId: asNumber(id), depth, parent, self: 0n, weight: 0n });
      parent = key;
    });
    const leaf = nodes.get(parent!)!;
    leaf.self += asBigInt(metric === 'alloc' ? chain.alloc_bytes : chain.self_ns);
  }
  const ordered = [...nodes.values()].sort((a, b) => b.depth - a.depth || a.key.localeCompare(b.key));
  for (const node of ordered) {
    node.weight += node.self > 0n ? node.self : 1n;
    if (node.parent) nodes.get(node.parent)!.weight += node.weight;
  }
  return [...nodes.values()].sort((a, b) => a.depth - b.depth || a.key.localeCompare(b.key));
}

export interface CriticalPath {
  segmentIds: number[];
  durationNs: bigint;
  complete: boolean;
}

/** Longest weighted path through explicitly captured causal segment edges. */
export function buildCriticalPath(root: ExactRoot): CriticalPath | null {
  if (root.execution.segments.length === 0) return null;
  const segments = new Map(root.execution.segments.map(segment => [asNumber(segment.id), segment]));
  const incoming = new Map<number, number[]>();
  const outgoing = new Map<number, number[]>();
  const degree = new Map<number, number>();
  for (const id of segments.keys()) degree.set(id, 0);
  for (const edge of root.execution.dependencies) {
    const from = asNumber(edge.from);
    const to = asNumber(edge.to);
    if (!segments.has(from) || !segments.has(to)) continue;
    incoming.set(to, [...(incoming.get(to) ?? []), from]);
    outgoing.set(from, [...(outgoing.get(from) ?? []), to]);
    degree.set(to, (degree.get(to) ?? 0) + 1);
  }
  const ready = [...degree].filter(([, value]) => value === 0).map(([id]) => id).sort((a, b) => a - b);
  const best = new Map<number, { duration: bigint; previous: number | null }>();
  let visited = 0;
  while (ready.length) {
    const id = ready.shift()!;
    visited += 1;
    const segment = segments.get(id)!;
    const weight = asBigInt(segment.t_end_ns) - asBigInt(segment.t_start_ns);
    let prior: { duration: bigint; previous: number | null } = { duration: 0n, previous: null };
    for (const parent of incoming.get(id) ?? []) {
      const candidate = best.get(parent);
      if (
        candidate &&
        (candidate.duration > prior.duration ||
          (candidate.duration === prior.duration && (prior.previous === null || parent < prior.previous)))
      ) prior = { duration: candidate.duration, previous: parent };
    }
    best.set(id, { duration: prior.duration + weight, previous: prior.previous });
    for (const child of outgoing.get(id) ?? []) {
      degree.set(child, (degree.get(child) ?? 1) - 1);
      if (degree.get(child) === 0) {
        ready.push(child);
        ready.sort((a, b) => a - b);
      }
    }
  }
  if (visited !== segments.size) return null;
  const end = [...best].sort((left, right) =>
    right[1].duration > left[1].duration ? 1 : right[1].duration < left[1].duration ? -1 : left[0] - right[0],
  )[0];
  if (!end) return null;
  const path: number[] = [];
  let current: number | null = end[0];
  while (current !== null) {
    path.push(current);
    current = best.get(current)?.previous ?? null;
  }
  path.reverse();
  return { segmentIds: path, durationNs: end[1].duration, complete: root.execution.status === 'complete' };
}
