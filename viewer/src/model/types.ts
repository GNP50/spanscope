import type { Profile } from '../generated/profile';
import type { RoutineSummary } from './routines';

/** JSON numbers above 2^53 arrive as `bigint`; every numeric field may be either. */
export type Exact<T> = T extends number
  ? number | bigint
  : T extends readonly (infer U)[]
    ? Exact<U>[]
    : T extends object
      ? { [K in keyof T]: Exact<T[K]> }
      : T;

export type ExactProfile = Exact<Profile>;
export type ExactChain = ExactProfile['chains'][number];
export type ExactRoot = ExactProfile['roots'][number];
export type ExactSpan = ExactProfile['spans'][number];
export type ExactObjects = NonNullable<ExactProfile['objects']>;
export type ExactInstance = ExactObjects['instances'][number];
export type ExactObjectType = ExactObjects['types'][number];
export type ExactFieldStats = ExactObjects['fields'][number];
export type ExactAccessSite = ExactObjects['sites'][number];

/** Per-type totals shown in the Objects view. */
export interface TypeSummary {
  typeId: number;
  instances: number;
  versions: bigint;
  changedInstances: number;
  approximate: number;
}

/** Lookup tables over the optional `objects` section. */
export interface ObjectsIndex {
  objects: ExactObjects;
  types: Map<number, ExactObjectType>;
  instances: Map<number, ExactInstance>;
  /** Instance IDs per type, most versions first. */
  instancesByType: Map<number, number[]>;
  /** Containment: parent instance → child instances. */
  children: Map<number, number[]>;
  /** Lineage: original instance → clones. */
  clones: Map<number, number[]>;
  /** Instances replaced by the key instance. */
  replaces: Map<number, number[]>;
  /** Instance IDs with counters for each span. */
  instancesBySpan: Map<number, number[]>;
  fieldsBySpan: Map<number, ExactFieldStats[]>;
  fieldsByType: Map<number, ExactFieldStats[]>;
  sitesByType: Map<number, ExactAccessSite[]>;
  typeSummaries: TypeSummary[];
}

export interface ProfileIndex {
  profile: ExactProfile;
  spans: Map<number, ExactSpan>;
  chains: Map<number, ExactChain>;
  roots: Map<string, ExactRoot>;
  totalCalls: bigint;
  totalSelfNs: bigint;
  /** Whole-profile routine aggregates, most exclusive time first. */
  routines: RoutineSummary[];
  objects: ObjectsIndex | null;
}

export interface Filters {
  query: string;
  spanId: number | null;
  rootUid: string | null;
}
