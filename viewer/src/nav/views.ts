/** The one list of views: order, labels, icons, and which use the shared filter bar. */
export const VIEWS = [
  { id: 'overview', label: 'Overview', icon: '◫', filters: true },
  { id: 'compare', label: 'Compare', icon: '±', filters: false },
  { id: 'insights', label: 'Insights', icon: '◇', filters: false },
  { id: 'patterns', label: 'Patterns', icon: '∴', filters: false },
  { id: 'timeline', label: 'Timeline', icon: '↦', filters: false },
  { id: 'routines', label: 'Routines', icon: '◉', filters: true },
  { id: 'runs', label: 'Runs & metrics', icon: '▦', filters: true },
  { id: 'dependencies', label: 'Dependencies', icon: '⌁', filters: true },
  { id: 'memory', label: 'Memory', icon: '▤', filters: false },
  { id: 'objects', label: 'Objects', icon: '◈', filters: false },
  { id: 'flame', label: 'Flame graph', icon: '▥', filters: true },
  { id: 'chains', label: 'Call chains', icon: '≡', filters: true },
  { id: 'raw', label: 'Raw explorer', icon: '{}', filters: false },
] as const;

export type View = (typeof VIEWS)[number]['id'];

export const isView = (value: string | null): value is View => VIEWS.some(view => view.id === value);

export const viewMeta = (id: View) => VIEWS.find(view => view.id === id)!;

/** Something the user can select and link to. */
export type Entity =
  | { kind: 'span'; id: number }
  | { kind: 'chain'; id: number }
  | { kind: 'root'; uid: string }
  | { kind: 'type'; id: number }
  | { kind: 'instance'; id: number };

/** Where a link to an entity opens when the caller does not choose a view. */
export const defaultView: Record<Entity['kind'], View> = {
  span: 'routines',
  chain: 'chains',
  root: 'runs',
  type: 'objects',
  instance: 'objects',
};
