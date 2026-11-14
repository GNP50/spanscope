import { asBigInt, asNumber, byBigIntDesc } from './format';
import type {
  ExactAccessSite,
  ExactFieldStats,
  ExactInstance,
  ExactObjects,
  ExactObjectType,
  ObjectsIndex,
  TypeSummary,
} from './types';

function push<K, V>(map: Map<K, V[]>, key: K, value: V) {
  const list = map.get(key);
  if (list) list.push(value);
  else map.set(key, [value]);
}

const isObject = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value);

/** Validate the references inside `objects` and build its lookup tables. */
export function indexObjects(objects: ExactObjects, spanIds: Set<number>): ObjectsIndex {
  for (const field of ['types', 'instances', 'fields', 'sites'] as const) {
    if (!Array.isArray(objects[field])) throw new Error(`Invalid objects section: ${field} must be an array.`);
  }
  const types = new Map<number, ExactObjectType>();
  for (const type of objects.types) {
    if (!isObject(type) || typeof type.name !== 'string') throw new Error('Invalid object type record.');
    const id = asNumber(type.id);
    if (types.has(id)) throw new Error(`Duplicate object type ${id}.`);
    types.set(id, type);
  }
  const instances = new Map<number, ExactInstance>();
  for (const instance of objects.instances) {
    if (!isObject(instance) || typeof instance.key !== 'string') throw new Error('Invalid object instance record.');
    const id = asNumber(instance.id);
    if (instances.has(id)) throw new Error(`Duplicate object instance ${id}.`);
    if (!types.has(asNumber(instance.type_id))) throw new Error(`Instance ${id} references a missing type.`);
    instances.set(id, instance);
  }
  const span = (value: number | bigint, what: string) => {
    if (!spanIds.has(asNumber(value))) throw new Error(`${what} references a missing span.`);
  };
  const instancesByType = new Map<number, number[]>();
  const children = new Map<number, number[]>();
  const clones = new Map<number, number[]>();
  const replaces = new Map<number, number[]>();
  const instancesBySpan = new Map<number, number[]>();
  const summaries = new Map<number, TypeSummary>();
  for (const [id, instance] of instances) {
    const typeId = asNumber(instance.type_id);
    for (const relation of ['parent', 'cloned_from', 'replaced_by'] as const) {
      const target = instance[relation];
      if (target !== null && target !== undefined && !instances.has(asNumber(target))) {
        throw new Error(`Instance ${id} references a missing ${relation.replace('_', ' ')} instance.`);
      }
    }
    push(instancesByType, typeId, id);
    if (instance.parent !== null && instance.parent !== undefined) push(children, asNumber(instance.parent), id);
    if (instance.cloned_from !== null && instance.cloned_from !== undefined) push(clones, asNumber(instance.cloned_from), id);
    if (instance.replaced_by !== null && instance.replaced_by !== undefined) push(replaces, asNumber(instance.replaced_by), id);
    for (const calls of instance.calls) {
      span(calls.span, `Instance ${id}`);
      push(instancesBySpan, asNumber(calls.span), id);
    }
    for (const version of instance.history) span(version.span, `Instance ${id} history`);
    const summary = summaries.get(typeId) ?? { typeId, instances: 0, versions: 0n, changedInstances: 0, approximate: 0 };
    summary.instances += 1;
    summary.versions += asBigInt(instance.versions);
    if (asBigInt(instance.versions) > 0n) summary.changedInstances += 1;
    if (instance.key_kind === 'address') summary.approximate += 1;
    summaries.set(typeId, summary);
  }
  const versions = (id: number) => asBigInt(instances.get(id)!.versions);
  for (const list of instancesByType.values()) list.sort((a, b) => byBigIntDesc(versions(a), versions(b)) || a - b);
  const fieldsBySpan = new Map<number, ExactFieldStats[]>();
  const fieldsByType = new Map<number, ExactFieldStats[]>();
  for (const field of objects.fields) {
    span(field.span, 'Field counter');
    if (!types.has(asNumber(field.type_id))) throw new Error('Field counter references a missing type.');
    push(fieldsBySpan, asNumber(field.span), field);
    push(fieldsByType, asNumber(field.type_id), field);
  }
  const sitesByType = new Map<number, ExactAccessSite[]>();
  for (const site of objects.sites) {
    span(site.span, 'Access site');
    if (!types.has(asNumber(site.type_id))) throw new Error('Access site references a missing type.');
    push(sitesByType, asNumber(site.type_id), site);
  }
  for (const id of types.keys()) {
    if (!summaries.has(id)) summaries.set(id, { typeId: id, instances: 0, versions: 0n, changedInstances: 0, approximate: 0 });
  }
  const typeSummaries = [...summaries.values()].sort(
    (a, b) => byBigIntDesc(a.versions, b.versions) || b.instances - a.instances || a.typeId - b.typeId,
  );
  return {
    objects,
    types,
    instances,
    instancesByType,
    children,
    clones,
    replaces,
    instancesBySpan,
    fieldsBySpan,
    fieldsByType,
    sitesByType,
    typeSummaries,
  };
}

/** One path of a type aggregated across functions, for the field table. */
export interface FieldRow {
  path: string;
  reads: bigint;
  writes: bigint;
  calls: bigint;
  changes: bigint;
  spans: ExactFieldStats[];
}

export function fieldRows(fields: ExactFieldStats[]): FieldRow[] {
  const rows = new Map<string, FieldRow>();
  for (const field of fields) {
    const row = rows.get(field.path) ?? { path: field.path, reads: 0n, writes: 0n, calls: 0n, changes: 0n, spans: [] };
    row.reads += asBigInt(field.reads);
    row.writes += asBigInt(field.writes);
    row.calls += asBigInt(field.calls);
    row.changes += asBigInt(field.changes);
    row.spans.push(field);
    rows.set(field.path, row);
  }
  return [...rows.values()].sort((a, b) => a.path.localeCompare(b.path));
}

/** Depth of a field path, for indentation: `items[*].price` is 2. */
export const pathDepth = (path: string): number => (path ? path.split(/\.|\[\*\]/).filter(Boolean).length - 1 : 0);
