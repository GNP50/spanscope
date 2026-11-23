import { describe, expect, it } from 'vitest';
import fixture from '../fixtures/example.json';
import objectsFixture from '../fixtures/objects.json';
import { asNumber, formatBytes, shortName, validateAndIndex } from '../src/model';
import { fieldRows, pathDepth } from '../src/model/objects';
import { layeredLayout, radialLayout } from '../src/graph/layout';
import { dependencyModel, objectModel } from '../src/graph/models';
import { hashFromLocation, locationFromHash, selectionFor } from '../src/nav/location';

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));

describe('layered layout', () => {
  const edges = [
    [0, 1],
    [0, 2],
    [1, 3],
    [2, 3],
    [3, 1],
  ] as const;
  it('puts callers above callees and breaks cycles deterministically', () => {
    const first = layeredLayout(4, edges);
    const second = layeredLayout(4, edges);
    expect([...first.x]).toEqual([...second.x]);
    expect([...first.y]).toEqual([...second.y]);
    expect(first.reversed.size).toBe(1);
    expect(first.layer[0]).toBe(0);
    expect(first.layer[3]).toBeGreaterThan(first.layer[1]);
  });
  it('places every node at a distinct finite position', () => {
    const layout = layeredLayout(4, edges);
    const points = new Set([...layout.x].map((x, node) => `${x}:${layout.y[node]}`));
    expect(points.size).toBe(4);
    expect([...layout.x, ...layout.y].every(Number.isFinite)).toBe(true);
  });
  it('handles empty graphs and self loops', () => {
    expect(layeredLayout(0, []).x.length).toBe(0);
    expect(layeredLayout(1, [[0, 0]]).layer[0]).toBe(0);
  });
});

describe('radial layout', () => {
  it('rings children outside parents and satellites outside the tree', () => {
    const parent = Int32Array.from([-1, 0, 0, -1, 3]);
    const group = Int32Array.from([0, 1, 1, 0, 1]);
    const layout = radialLayout(parent, group, 2, [
      [0, 0],
      [1, 3],
    ]);
    const radius = (node: number) => Math.hypot(layout.x[node], layout.y[node]);
    expect(radius(1)).toBeGreaterThan(radius(0));
    expect(radius(4)).toBeGreaterThan(radius(3));
    expect(radius(5)).toBeGreaterThan(radius(1));
    expect(radius(6)).toBeGreaterThan(radius(4));
    expect([...layout.x, ...layout.y].every(Number.isFinite)).toBe(true);
  });
  it('survives containment cycles', () => {
    const layout = radialLayout(Int32Array.from([1, 0]), Int32Array.from([0, 0]), 0, []);
    expect([...layout.x, ...layout.y].every(Number.isFinite)).toBe(true);
  });
});

describe('objects index', () => {
  const index = validateAndIndex(objectsFixture);
  const objects = index.objects!;
  it('indexes types, instances, containment and lineage', () => {
    expect(objects.types.size).toBe(2);
    expect(objects.instances.size).toBe(objectsFixture.objects.instances.length);
    const cart = [...objects.types].find(([, type]) => type.name.endsWith('::Cart'))![0];
    expect(objects.instancesByType.get(cart)!.length).toBe(7);
    const clones = [...objects.clones.values()].flat();
    expect(clones.length).toBe(1);
    const first = objects.instancesByType.get(cart)![0];
    expect(objects.children.get(first)?.length).toBeGreaterThan(0);
    const versions = objects.instancesByType.get(cart)!.map(id => asNumber(objects.instances.get(id)!.versions));
    expect(versions).toEqual([...versions].sort((a, b) => b - a));
  });
  it('aggregates field counters per path', () => {
    const cart = [...objects.types].find(([, type]) => type.name.endsWith('::Cart'))![0];
    const rows = fieldRows(objects.fieldsByType.get(cart)!);
    const total = rows.find(row => row.path === 'total')!;
    expect(total.writes).toBeGreaterThan(0n);
    expect(total.spans.length).toBeGreaterThan(1);
    expect(pathDepth('items[*].price')).toBe(1);
    expect(pathDepth('')).toBe(0);
  });
  it('keeps profiles without objects valid', () => {
    expect(validateAndIndex(fixture).objects).toBeNull();
  });
  it('rejects dangling object references', () => {
    const broken = clone(objectsFixture);
    broken.objects.instances[0].type_id = 99;
    expect(() => validateAndIndex(broken)).toThrow(/missing type/);
    const orphan = clone(objectsFixture);
    orphan.objects.instances[0].parent = 9999;
    expect(() => validateAndIndex(orphan)).toThrow(/missing parent/);
    const span = clone(objectsFixture);
    span.objects.fields[0].span = 9999;
    expect(() => validateAndIndex(span)).toThrow(/missing span/);
  });
});

describe('graph models', () => {
  it('builds a call graph scene with one node per routine', () => {
    const index = validateAndIndex(fixture);
    const model = dependencyModel(index, index.routines);
    expect(model.scene.nodeCount).toBe(index.routines.length);
    expect(model.scene.edgeCount).toBe(index.routines.reduce((sum, row) => sum + row.callers.length, 0));
    expect(model.lookup.get(`span:${index.routines[0].spanId}`)).toBe(0);
  });
  it('builds an object graph with instances and the functions that touched them', () => {
    const index = validateAndIndex(objectsFixture);
    const model = objectModel(index)!;
    const functions = index.objects!.instancesBySpan.size;
    expect(model.scene.nodeCount).toBe(index.objects!.instances.size + functions);
    expect(model.entities.filter(entity => entity.kind === 'span').length).toBe(functions);
    expect(objectModel(validateAndIndex(fixture))).toBeNull();
  });
});

describe('navigation', () => {
  it('round-trips the selection through the URL hash', () => {
    const location = { view: 'objects' as const, query: 'cart', spanId: 3, chainId: 4, rootUid: 'abc', typeId: 1, instanceId: 7 };
    expect(locationFromHash(hashFromLocation(location))).toEqual(location);
    expect(locationFromHash('#view=nope').view).toBe('overview');
  });
  it('derives related selection from entities', () => {
    const index = validateAndIndex(objectsFixture);
    const [id, instance] = [...index.objects!.instances][0];
    expect(selectionFor({ kind: 'instance', id }, index)).toEqual({ instanceId: id, typeId: asNumber(instance.type_id) });
    const chain = index.profile.chains[0];
    expect(selectionFor({ kind: 'chain', id: asNumber(chain.id) }, index).spanId).toBe(asNumber(chain.path[chain.path.length - 1]));
  });
});

describe('format helpers', () => {
  it('formats bytes and short names', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(2048)).toBe('2.0 KiB');
    expect(shortName('a::b::c')).toBe('b::c');
  });
});
