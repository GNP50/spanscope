import { asNumber } from './format';
import type { ProfileIndex } from './types';

/** Typed, defensive views of the `ml` analysis sections; section data is draft JSON. */

export interface SectionInfo {
  status: string;
  explanation: string;
}

export interface Cluster {
  id: number;
  size: number;
  medianNs: number;
  roots: string[];
  distinguishing: { span: number; share: number; delta: number }[];
}

export interface Outlier {
  root: string;
  z: number;
  cluster: number;
  durationNs: number;
  typicalNs: number;
}

export interface Changepoint {
  index: number;
  root: string;
  tNs: number;
  z: number;
  beforeNs: number;
  afterNs: number;
}

export interface GrowthFit {
  metric: string;
  span: number | null;
  model: string | null;
  r2: number;
  fits: { model: string; r2: number }[];
  points: [number, number, string][];
}

export interface Churn {
  instance: number;
  typeId: number;
  z: number;
  versions: number;
  typical: number;
}

export interface Patterns {
  clusters: (SectionInfo & { silhouette: number | null; clusters: Cluster[]; assignment: Map<string, number> }) | null;
  anomalies: (SectionInfo & { runs: Outlier[] }) | null;
  changepoints: (SectionInfo & { points: Changepoint[] }) | null;
  complexity: (SectionInfo & { fits: GrowthFit[] }) | null;
  churn: (SectionInfo & { instances: Churn[] }) | null;
}

type Json = Record<string, unknown>;
const object = (value: unknown): Json => (value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Json) : {});
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const num = (value: unknown): number => (typeof value === 'number' || typeof value === 'bigint' ? asNumber(value) : 0);
const str = (value: unknown): string => (typeof value === 'string' ? value : '');

export function parsePatterns(index: ProfileIndex): Patterns | null {
  const sections = index.profile.analysis.sections;
  const keys = ['clusters', 'anomalies', 'changepoints', 'complexity', 'object_churn'];
  if (!keys.some(key => sections[key])) return null;
  const info = (key: string): (SectionInfo & { data: Json }) | null => {
    const section = sections[key];
    return section ? { status: section.status, explanation: section.explanation, data: object(section.data) } : null;
  };
  const clusters = info('clusters');
  const anomalies = info('anomalies');
  const changepoints = info('changepoints');
  const complexity = info('complexity');
  const churn = info('object_churn');
  const strip = <T extends { data: Json }>(section: T): Omit<T, 'data'> => {
    const { data: _data, ...rest } = section;
    return rest;
  };
  return {
    clusters: clusters && {
      ...strip(clusters),
      silhouette: typeof clusters.data.silhouette === 'number' ? clusters.data.silhouette : null,
      clusters: list(clusters.data.clusters).map(entry => {
        const cluster = object(entry);
        return {
          id: num(cluster.id),
          size: num(cluster.size),
          medianNs: num(cluster.median_duration_ns),
          roots: list(cluster.roots).map(str),
          distinguishing: list(cluster.distinguishing).map(item => {
            const trait = object(item);
            return { span: num(trait.span), share: num(trait.share), delta: num(trait.delta) };
          }),
        };
      }),
      assignment: new Map(
        list(clusters.data.assignment).map(pair => {
          const [uid, cluster] = list(pair);
          return [str(uid), num(cluster)] as const;
        }),
      ),
    },
    anomalies: anomalies && {
      ...strip(anomalies),
      runs: list(anomalies.data.runs).map(entry => {
        const run = object(entry);
        return { root: str(run.root), z: num(run.z), cluster: num(run.cluster), durationNs: num(run.duration_ns), typicalNs: num(run.typical_ns) };
      }),
    },
    changepoints: changepoints && {
      ...strip(changepoints),
      points: list(changepoints.data.changepoints).map(entry => {
        const point = object(entry);
        return {
          index: num(point.index),
          root: str(point.root),
          tNs: num(point.t_ns),
          z: num(point.z),
          beforeNs: num(point.before_median_ns),
          afterNs: num(point.after_median_ns),
        };
      }),
    },
    complexity: complexity && {
      ...strip(complexity),
      fits: list(complexity.data.fits).map(entry => {
        const fit = object(entry);
        return {
          metric: str(fit.metric),
          span: fit.span === null || fit.span === undefined ? null : num(fit.span),
          model: typeof fit.model === 'string' ? fit.model : null,
          r2: num(fit.r2),
          fits: list(fit.fits).map(item => ({ model: str(object(item).model), r2: num(object(item).r2) })),
          points: list(fit.points).map(point => {
            const [x, y, uid] = list(point);
            return [num(x), num(y), str(uid)] as [number, number, string];
          }),
        };
      }),
    },
    churn: churn && {
      ...strip(churn),
      instances: list(churn.data.instances).map(entry => {
        const item = object(entry);
        return { instance: num(item.instance), typeId: num(item.type_id), z: num(item.z), versions: num(item.versions), typical: num(item.typical_versions) };
      }),
    },
  };
}

const GROWTH: Record<string, (n: number) => number> = {
  'O(log n)': n => Math.log(n),
  'O(n)': n => n,
  'O(n log n)': n => n * Math.log(n),
  'O(n²)': n => n * n,
};

/** Least-squares curve `a + b·f(x)` for drawing a reported growth model. */
export function fittedCurve(points: [number, number, string][], model: string): [number, number][] {
  const f = GROWTH[model];
  if (!f || points.length < 2) return [];
  const fx = points.map(([x]) => f(x));
  const mx = fx.reduce((sum, value) => sum + value, 0) / fx.length;
  const my = points.reduce((sum, [, y]) => sum + y, 0) / points.length;
  const sxy = fx.reduce((sum, value, i) => sum + (value - mx) * (points[i][1] - my), 0);
  const sxx = fx.reduce((sum, value) => sum + (value - mx) ** 2, 0);
  if (sxx === 0) return [];
  const b = sxy / sxx;
  const a = my - b * mx;
  return [...new Set(points.map(([x]) => x))].sort((l, r) => l - r).map(x => [x, a + b * f(x)]);
}
