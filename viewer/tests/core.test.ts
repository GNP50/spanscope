import { describe, expect, it } from 'vitest';
import fixture from '../fixtures/example.json';
import asyncFixture from '../../spanscope/tests/fixtures/async-partial.json';
import { asBigInt, buildCriticalPath, buildFlame, filterChains, parseLosslessJson, validateAndIndex } from '../src/core';
import { metricNames, summarizeRoutines } from '../src/insights';
import { compareProfiles } from '../src/comparison';

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
    const transformChain = index.profile.chains.find(chain => Number(chain.path[chain.path.length - 1]) === transform.spanId)!;
    expect(transform.calls).toBe(880n);
    expect(transform.callers).toEqual([{ spanId: 0, calls: 880n, inclusiveWallNs: asBigInt(transformChain.total_ns) }]);
    expect(all.find(row => row.spanId === 0)?.callees[0].calls).toBe(880n);
    const first = summarizeRoutines(index, fixture.roots[0].uid);
    const localTransform = first.find(row => row.spanId === transform.spanId)!;
    expect(localTransform.calls).toBe(200n);
    expect(localTransform.callers[0].calls).toBe(200n);
    const rootSelf = fixture.roots[0].chain_self_ns as Record<string, number>;
    expect(localTransform.selfActiveNs).toBe(asBigInt(rootSelf[String(transformChain.id)]));
    expect(metricNames(index)).toEqual(['input_size']);
  });
});

describe('profile comparison', () => {
  it('matches routines across profile-local ids and reports regressions', () => {
    const baseline = validateAndIndex(fixture);
    const candidateProfile = structuredClone(fixture);
    candidateProfile.chains[1].self_ns *= 2;
    candidateProfile.chains[1].alloc_bytes = 4096;
    const candidate = validateAndIndex(candidateProfile);
    const comparison = compareProfiles(baseline, candidate);
    const transform = comparison.rows.find(row => row.name === 'demo::transform')!;
    expect(transform.baseline?.calls).toBe(880n);
    expect(transform.candidate?.selfNs).toBe(transform.baseline!.selfNs * 2n);
    expect(transform.candidate?.p99Ns).toBe(asBigInt(fixture.chains[1].p99_ns));
    expect(transform.candidate?.allocBytes).toBe(4096n);
    expect(comparison.regressions).toBe(1);
    expect(comparison.added).toBe(0);
    expect(comparison.removed).toBe(0);
  });
});

describe('causal critical path', () => {
  it('uses only explicit segment dependencies', () => {
    const root = validateAndIndex(asyncFixture).profile.roots[0];
    const path = buildCriticalPath(root)!;
    expect(path.segmentIds).toEqual([0, 1]);
    expect(path.durationNs).toBe(20n);
    expect(path.complete).toBe(false);
  });
});
