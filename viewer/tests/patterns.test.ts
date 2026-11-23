import { describe, expect, it } from 'vitest';
import fixture from '../fixtures/example.json';
import patternsFixture from '../fixtures/patterns.json';
import { validateAndIndex } from '../src/model';
import { fittedCurve, parsePatterns } from '../src/model/patterns';

describe('pattern sections', () => {
  const index = validateAndIndex(patternsFixture);
  const patterns = parsePatterns(index)!;
  it('reads clusters, outliers, shifts, growth and churn from the ml sections', () => {
    expect(patterns.clusters?.clusters.length).toBeGreaterThanOrEqual(2);
    const sizes = patterns.clusters!.clusters.reduce((sum, cluster) => sum + cluster.size, 0);
    expect(sizes).toBe(index.profile.roots.length);
    expect(patterns.clusters!.assignment.size).toBe(index.profile.roots.length);
    const medians = patterns.clusters!.clusters.map(cluster => cluster.medianNs);
    expect(medians).toEqual([...medians].sort((a, b) => a - b));
    expect(patterns.anomalies?.runs.every(run => index.roots.has(run.root))).toBe(true);
    expect(patterns.changepoints?.points.length).toBeGreaterThan(0);
    expect(patterns.complexity?.fits.some(fit => fit.model !== null)).toBe(true);
    expect(patterns.churn?.instances.every(item => index.objects?.instances.has(item.instance))).toBe(true);
  });
  it('is absent for profiles exported without the ml feature', () => {
    expect(parsePatterns(validateAndIndex(fixture))).toBeNull();
  });
  it('draws the least-squares curve of a reported model', () => {
    const points: [number, number, string][] = [1, 2, 3, 4].map(x => [x, 3 + 2 * x * x, `r${x}`]);
    const curve = fittedCurve(points, 'O(n²)');
    expect(curve.map(([x]) => x)).toEqual([1, 2, 3, 4]);
    curve.forEach(([x, y]) => expect(y).toBeCloseTo(3 + 2 * x * x));
    expect(fittedCurve(points, 'unknown')).toEqual([]);
  });
});
