import { describe, expect, it } from 'vitest';
import fixture from '../fixtures/example.json';
import { asBigInt, buildFlame, filterChains, parseLosslessJson, validateAndIndex } from '../src/core';
import { metricNames, summarizeRoutines } from '../src/insights';

describe('lossless parser', () => {
  it('preserves unsafe positive and negative counters without altering strings', () => {
    const parsed = parseLosslessJson('{"large":9007199254740993,"negative":-9007199254740993,"safe":9007199254740991,"name":"9007199254740993","escaped":"\\\"-9007199254740993"}') as Record<string, unknown>;
    expect(parsed.large).toBe(9007199254740993n);
    expect(parsed.negative).toBe(-9007199254740993n);
    expect(parsed.safe).toBe(9007199254740991);
    expect(parsed.name).toBe('9007199254740993');
    expect(parsed.escaped).toBe('"-9007199254740993');
  });
  it('does not confuse a profile string resembling the sentinel', () => {
    const parsed = parseLosslessJson('{"text":"__SPANSCOPE_EXACT_INTEGER__9007199254740993","count":9007199254740993}') as Record<string, unknown>;
    expect(parsed.text).toBe('__SPANSCOPE_EXACT_INTEGER__9007199254740993');
    expect(parsed.count).toBe(9007199254740993n);
  });
});

describe('profile index and filters', () => {
  const index = validateAndIndex(fixture);
  it('indexes the real exported fixture', () => {
    expect(index.spans.size).toBe(2);
    expect(index.chains.size).toBe(2);
    expect(index.roots.size).toBe(5);
    expect(index.totalCalls).toBeGreaterThan(0n);
  });
  it('filters linked span, root, and name selections', () => {
    const all = filterChains(index, { query: '', spanId: null, rootUid: null });
    const leaf = index.profile.spans.find(span => span.name.includes('transform'))!;
    const bySpan = filterChains(index, { query: '', spanId: Number(leaf.id), rootUid: null });
    expect(bySpan.length).toBe(1);
    expect(filterChains(index, { query: 'transform', spanId: null, rootUid: null })).toEqual(bySpan);
    const root = index.profile.roots[0];
    const byRoot = filterChains(index, { query: '', spanId: null, rootUid: root.uid });
    expect(byRoot.length).toBeGreaterThan(0);
    expect(byRoot.length).toBeLessThanOrEqual(all.length);
  });
  it('builds additive structural flame weights', () => {
    const nodes = buildFlame(index.profile.chains);
    expect(nodes.length).toBeGreaterThan(0);
    expect(nodes.filter(node => node.depth === 0).reduce((sum, node) => sum + node.weight, 0n)).toBeGreaterThanOrEqual(asBigInt(index.profile.chains[0].self_ns));
  });
  it('rejects unsupported version and dangling chain identity', () => {
    expect(() => validateAndIndex({ ...fixture, schema_version: 2 })).toThrow(/schema_version/);
    expect(() => validateAndIndex({ ...fixture, chains: [{ ...fixture.chains[0], path: [999] }] })).toThrow(/chain path/);
  });
  it('answers who called a routine and how often in the capture and a single run', () => {
    const all = summarizeRoutines(index, null);
    const transform = all.find(row => index.spans.get(row.spanId)?.name === 'demo::transform')!;
    expect(transform.calls).toBe(880n);
    expect(transform.callers).toEqual([{ spanId: 0, calls: 880n, inclusiveWallNs: 81482n }]);
    expect(all.find(row => row.spanId === 0)?.callees[0].calls).toBe(880n);
    const first = summarizeRoutines(index, fixture.roots[0].uid);
    const localTransform = first.find(row => row.spanId === transform.spanId)!;
    expect(localTransform.calls).toBe(200n);
    expect(localTransform.callers[0].calls).toBe(200n);
    expect(localTransform.selfActiveNs).toBe(19765n);
    expect(metricNames(index)).toEqual(['input_size']);
  });
});
