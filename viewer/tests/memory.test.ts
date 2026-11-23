import { describe, expect, it } from 'vitest';
import fixture from '../fixtures/example.json';
import memoryFixture from '../fixtures/memory.json';
import {
  buildFlame,
  formatBytes,
  hasHeapData,
  memoryRoutines,
  memoryTotals,
  memoryTree,
  peakHistogram,
  rootMemorySeries,
  spanShortName,
  validateAndIndex,
} from '../src/model';

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));

describe('heap balance model', () => {
  const index = validateAndIndex(clone(memoryFixture));

  it('detects heap data only when the capture says so', () => {
    expect(hasHeapData(index.profile)).toBe(true);
    expect(hasHeapData(validateAndIndex(clone(fixture)).profile)).toBe(false);
  });

  it('aggregates routines consistently with chain records', () => {
    const rows = memoryRoutines(index);
    expect(rows.length).toBe(index.profile.chains.length);
    for (const row of rows) {
      expect(row.inclAllocBytes).toBeGreaterThanOrEqual(row.allocBytes);
      expect(row.netBytes).toBe(row.inclAllocBytes - row.inclFreedBytes);
      expect(row.peakMax).toBeLessThanOrEqual(row.inclAllocBytes);
      expect(row.netMin).toBeLessThanOrEqual(row.netMax);
    }
    // Largest peak first.
    expect(rows[0].peakMax).toBeGreaterThanOrEqual(rows[rows.length - 1].peakMax);
  });

  it('counts totals from top-level chains only', () => {
    const totals = memoryTotals(index);
    const top = index.profile.chains.filter(chain => chain.path.length === 1);
    const expected = top.reduce((sum, chain) => sum + BigInt(chain.memory!.incl_alloc_bytes), 0n);
    expect(totals.allocBytes).toBe(expected);
    expect(totals.netBytes).toBe(totals.allocBytes - totals.freedBytes);
    expect(totals.retainingRoutines).toBeGreaterThan(0);
  });

  it('builds a cumulative run series and per-routine peak histograms', () => {
    const series = rootMemorySeries(index);
    expect(series.length).toBe(index.profile.roots.length);
    let running = 0n;
    for (const point of series) {
      running += point.netBytes;
      expect(point.cumulativeNetBytes).toBe(running);
    }
    const spanId = memoryRoutines(index)[0].spanId;
    const calls = memoryRoutines(index)[0].calls;
    expect(peakHistogram(index, spanId).reduce((sum, bucket) => sum + bucket.count, 0n)).toBe(calls);
  });

  it('keeps sunburst children within their parent', () => {
    const tree = memoryTree(index.profile.chains, id => spanShortName(index, id));
    const check = (node: (typeof tree)[number]) => {
      expect(node.children.reduce((sum, child) => sum + child.value, 0)).toBeLessThanOrEqual(node.value);
      node.children.forEach(check);
    };
    expect(tree.length).toBeGreaterThan(0);
    tree.forEach(check);
  });

  it('weights the flame graph by allocated bytes on request', () => {
    const nodes = buildFlame(index.profile.chains, 'alloc');
    const top = nodes.filter(node => node.depth === 0);
    const total = index.profile.chains.reduce((sum, chain) => sum + BigInt(chain.alloc_bytes), 0n);
    expect(top.reduce((sum, node) => sum + node.weight, 0n)).toBeGreaterThanOrEqual(total);
  });

  it('formats negative and fractional byte counts', () => {
    expect(formatBytes(-2048)).toBe('−2.0 KiB');
    expect(formatBytes(10.6)).toBe('11 B');
  });
});
