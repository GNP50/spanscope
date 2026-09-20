import type { Profile } from './generated/profile';

export type Exact<T> = T extends number ? number | bigint : T extends readonly (infer U)[] ? Exact<U>[] : T extends object ? { [K in keyof T]: Exact<T[K]> } : T;
export type ExactProfile = Exact<Profile>;
export type ExactChain = ExactProfile['chains'][number];
export type ExactRoot = ExactProfile['roots'][number];
export type ExactSpan = ExactProfile['spans'][number];

export interface ProfileIndex {
  profile: ExactProfile;
  spans: Map<number, ExactSpan>;
  chains: Map<number, ExactChain>;
  roots: Map<string, ExactRoot>;
  bySpan: Map<number, number[]>;
  totalCalls: bigint;
  totalSelfNs: bigint;
}

export interface Filters { query: string; spanId: number | null; rootUid: string | null; }

const MAX_SAFE = BigInt(Number.MAX_SAFE_INTEGER);

/** Keep large JSON integer tokens exact before JSON.parse can round them. */
export function parseLosslessJson(text: string): unknown {
  let marker = '__SPANSCOPE_EXACT_INTEGER__';
  while (text.includes(marker)) marker += '_';
  let output = '';
  let quoted = false;
  let escaped = false;
  for (let i = 0; i < text.length;) {
    const character = text[i];
    if (quoted) {
      output += character;
      if (escaped) escaped = false;
      else if (character === '\\') escaped = true;
      else if (character === '"') quoted = false;
      i += 1;
      continue;
    }
    if (character === '"') { quoted = true; output += character; i += 1; continue; }
    if (character === '-' || (character >= '0' && character <= '9')) {
      const match = text.slice(i).match(/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/);
      if (!match) throw new Error(`Invalid number at ${i}`);
      const token = match[0];
      if (!/[.eE]/.test(token) && (token.length > 15 || token.length === 16 && token[0] === '-')) {
        const integer = BigInt(token);
        output += integer > MAX_SAFE || integer < -MAX_SAFE ? `"${marker}${token}"` : token;
      } else output += token;
      i += token.length;
      continue;
    }
    output += character;
    i += 1;
  }
  return JSON.parse(output, (_key, value: unknown) => {
    if (typeof value === 'string' && value.startsWith(marker)) {
      const digits = value.slice(marker.length);
      if (/^-?\d+$/.test(digits)) return BigInt(digits);
    }
    return value;
  });
}

export const asNumber = (value: number | bigint): number => typeof value === 'bigint' ? Number(value) : value;
export const asBigInt = (value: number | bigint): bigint => typeof value === 'bigint' ? value : BigInt(value);
export const formatInteger = (value: number | bigint): string => asBigInt(value).toLocaleString('en-US');
export const formatNs = (value: number | bigint): string => {
  const number = asNumber(value);
  if (number >= 1e9) return `${(number / 1e9).toFixed(2)} s`;
  if (number >= 1e6) return `${(number / 1e6).toFixed(2)} ms`;
  if (number >= 1e3) return `${(number / 1e3).toFixed(1)} µs`;
  return `${formatInteger(value)} ns`;
};
export const exactJson = (value: unknown): string => JSON.stringify(value, (_key, entry: unknown) => typeof entry === 'bigint' ? entry.toString() : entry, 2);

function isObject(value: unknown): value is Record<string, unknown> { return value !== null && typeof value === 'object' && !Array.isArray(value); }

/** Validate the versioned envelope and every reference used by the MVP. */
export function validateAndIndex(value: unknown): ProfileIndex {
  if (!isObject(value) || value.schema_version !== 1) throw new Error('Unsupported or missing spanscope schema_version (expected 1).');
  for (const field of ['spans', 'chains', 'roots', 'threads']) {
    if (!Array.isArray(value[field])) throw new Error(`Invalid profile: ${field} must be an array.`);
  }
  if (!isObject(value.meta) || !isObject(value.graph) || !isObject(value.analysis)) throw new Error('Invalid profile envelope.');
  const profile = value as ExactProfile;
  const spans = new Map<number, ExactSpan>();
  for (const span of profile.spans) {
    if (!isObject(span) || !Number.isSafeInteger(span.id) || typeof span.name !== 'string') throw new Error('Invalid span record.');
    if (spans.has(span.id as number)) throw new Error(`Duplicate span ${span.id}.`);
    spans.set(span.id as number, span);
  }
  const chains = new Map<number, ExactChain>();
  const bySpan = new Map<number, number[]>();
  let totalCalls = 0n;
  let totalSelfNs = 0n;
  for (const chain of profile.chains) {
    if (!isObject(chain) || !Number.isSafeInteger(chain.id) || !Array.isArray(chain.path) || chain.path.length === 0 || !chain.path.every(id => spans.has(asNumber(id)))) throw new Error('Invalid chain path.');
    const id = chain.id as number;
    if (chains.has(id)) throw new Error(`Duplicate chain ${id}.`);
    chains.set(id, chain);
    const leaf = asNumber(chain.path[chain.path.length - 1]);
    bySpan.set(leaf, [...(bySpan.get(leaf) ?? []), id]);
    totalCalls += asBigInt(chain.count);
    totalSelfNs += asBigInt(chain.self_ns);
  }
  const threadIds = new Set(profile.threads.map(thread => asNumber(thread.id)));
  const roots = new Map<string, ExactRoot>();
  for (const root of profile.roots) {
    if (!isObject(root) || typeof root.uid !== 'string' || !spans.has(asNumber(root.span)) || !threadIds.has(asNumber(root.thread))) throw new Error('Invalid root identity.');
    if (roots.has(root.uid)) throw new Error(`Duplicate root ${root.uid}.`);
    if (!Array.isArray(root.chains) || !root.chains.every(triple => Array.isArray(triple) && chains.has(asNumber(triple[0])))) throw new Error('Invalid root chain reference.');
    roots.set(root.uid, root);
  }
  return { profile, spans, chains, roots, bySpan, totalCalls, totalSelfNs };
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

export interface FlameNode { key: string; spanId: number; depth: number; parent: string | null; self: bigint; weight: bigint; }
export function buildFlame(chains: ExactChain[]): FlameNode[] {
  const nodes = new Map<string, FlameNode>();
  for (const chain of chains) {
    let parent: string | null = null;
    chain.path.forEach((id, depth) => {
      const key = chain.path.slice(0, depth + 1).join('/');
      if (!nodes.has(key)) nodes.set(key, { key, spanId: asNumber(id), depth, parent, self: 0n, weight: 0n });
      parent = key;
    });
    const leaf = nodes.get(parent!)!;
    leaf.self += asBigInt(chain.self_ns);
  }
  const ordered = [...nodes.values()].sort((a, b) => b.depth - a.depth || a.key.localeCompare(b.key));
  for (const node of ordered) {
    node.weight += node.self > 0n ? node.self : 1n;
    if (node.parent) nodes.get(node.parent)!.weight += node.weight;
  }
  return [...nodes.values()].sort((a, b) => a.depth - b.depth || a.key.localeCompare(b.key));
}
