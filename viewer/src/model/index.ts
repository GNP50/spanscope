import { asBigInt, asNumber } from './format';
import { indexObjects } from './objects';
import { summarizeRoutines } from './routines';
import type { ExactChain, ExactObjects, ExactProfile, ExactRoot, ExactSpan, ProfileIndex } from './types';

export * from './format';
export * from './lossless';
export * from './queries';
export * from './memory';
export type * from './types';

const isObject = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);

/** Validate the versioned envelope and every cross-reference the viewer follows. */
export function validateAndIndex(value: unknown): ProfileIndex {
  if (!isObject(value) || value.schema_version !== 1) throw new Error('Unsupported or missing spanscope schema_version (expected 1).');
  for (const field of ['spans', 'chains', 'roots', 'threads']) {
    if (!Array.isArray(value[field])) throw new Error(`Invalid profile: ${field} must be an array.`);
  }
  if (!isObject(value.meta) || !isObject(value.graph) || !isObject(value.analysis)) throw new Error('Invalid profile envelope.');
  if (value.objects !== undefined && value.objects !== null && !isObject(value.objects)) throw new Error('Invalid objects section.');
  const profile = value as ExactProfile;
  const spans = new Map<number, ExactSpan>();
  for (const span of profile.spans) {
    if (!isObject(span) || !Number.isSafeInteger(span.id) || typeof span.name !== 'string') throw new Error('Invalid span record.');
    if (spans.has(span.id as number)) throw new Error(`Duplicate span ${span.id}.`);
    spans.set(span.id as number, span);
  }
  const chains = new Map<number, ExactChain>();
  let totalCalls = 0n;
  let totalSelfNs = 0n;
  for (const chain of profile.chains) {
    if (
      !isObject(chain) ||
      !Number.isSafeInteger(chain.id) ||
      !Array.isArray(chain.path) ||
      chain.path.length === 0 ||
      !chain.path.every(id => spans.has(asNumber(id)))
    ) throw new Error('Invalid chain path.');
    const id = chain.id as number;
    if (chains.has(id)) throw new Error(`Duplicate chain ${id}.`);
    chains.set(id, chain);
    totalCalls += asBigInt(chain.count);
    totalSelfNs += asBigInt(chain.self_ns);
  }
  const threadIds = new Set(profile.threads.map(thread => asNumber(thread.id)));
  const roots = new Map<string, ExactRoot>();
  for (const root of profile.roots) {
    if (!isObject(root) || typeof root.uid !== 'string' || !spans.has(asNumber(root.span)) || !threadIds.has(asNumber(root.thread))) {
      throw new Error('Invalid root identity.');
    }
    if (roots.has(root.uid)) throw new Error(`Duplicate root ${root.uid}.`);
    if (!Array.isArray(root.chains) || !root.chains.every(triple => Array.isArray(triple) && chains.has(asNumber(triple[0])))) {
      throw new Error('Invalid root chain reference.');
    }
    roots.set(root.uid, root);
  }
  const objects = profile.objects ? indexObjects(profile.objects as ExactObjects, new Set(spans.keys())) : null;
  const index: ProfileIndex = { profile, spans, chains, roots, totalCalls, totalSelfNs, routines: [], objects };
  index.routines = summarizeRoutines(index, null);
  return index;
}
