import { packColor, SceneBuilder, tooltipCard, type Scene, type SceneStyles } from 'skeinmap';
import {
  asBigInt,
  asNumber,
  formatInteger,
  formatNs,
  instanceName,
  shortName,
  spanName,
  spanShortName,
  type ProfileIndex,
} from '../model';
import type { RoutineSummary } from '../model/routines';
import type { Entity } from '../nav/views';
import type { ThemeColors } from '../ui/theme';
import { layeredLayout, radialLayout } from './layout';

/** Node style bytes. Kinds 0–1 are resting states; 2–4 express selection. */
export const NODE = { function: 0, instance: 1, selected: 2, neighbor: 3, dim: 4 } as const;
/** Edge kinds; `kind + HIGHLIGHT` is the emphasized variant and `DIM_EDGE` the faded one. */
export const EDGE = { call: 0, contain: 1, clone: 2, replaced: 3, touch: 4, changed: 5 } as const;
const HIGHLIGHT = 6;
/** Node count up to which the object graph uses the layered layout. */
const LAYERED_LIMIT = 400;
const DIM_EDGE = 12;

export interface GraphModel {
  scene: Scene & { nodeColor: Uint32Array; nodeRadius: Float32Array; edgeWidth: Float32Array };
  /** Resting node style (`NODE.function` or `NODE.instance`). */
  kind: Uint8Array;
  /** Palette slot per node; -1 uses the accent color. */
  slot: Int16Array;
  edgeKind: Uint8Array;
  neighbors: number[][];
  entities: Entity[];
  labels: string[];
  priority: Float32Array;
  nodeTips: string[];
  edgeTips: string[];
  /** Node index of an entity key such as `span:3`. */
  lookup: Map<string, number>;
}

export const entityKey = (entity: Entity): string =>
  entity.kind === 'root' ? `root:${entity.uid}` : `${entity.kind}:${entity.id}`;

function finish(
  builder: SceneBuilder,
  kind: number[],
  slot: number[],
  radius: number[],
  edgeKind: number[],
  edgeWidth: number[],
  rest: Omit<GraphModel, 'scene' | 'kind' | 'slot' | 'edgeKind' | 'neighbors' | 'lookup'>,
): GraphModel {
  const base = builder.build();
  const scene = Object.assign(base, {
    nodeColor: new Uint32Array(base.nodeCount),
    nodeRadius: Float32Array.from(radius),
    edgeWidth: Float32Array.from(edgeWidth),
  });
  const neighbors: number[][] = Array.from({ length: base.nodeCount }, () => []);
  for (let edge = 0; edge < base.edgeCount; edge += 1) {
    const from = base.edgeFrom[edge];
    const to = base.edgeTo[edge];
    neighbors[from].push(to);
    neighbors[to].push(from);
  }
  const lookup = new Map(rest.entities.map((entity, node) => [entityKey(entity), node]));
  return {
    scene,
    kind: Uint8Array.from(kind),
    slot: Int16Array.from(slot),
    edgeKind: Uint8Array.from(edgeKind),
    neighbors,
    lookup,
    ...rest,
  };
}

const logSize = (value: bigint, min: number, max: number, scale: number) =>
  Math.max(min, Math.min(max, min + Math.log10(Number(value) + 1) * scale));

/** Caller → callee graph of routines, laid out in layers. */
export function dependencyModel(index: ProfileIndex, routines: RoutineSummary[]): GraphModel {
  const node = new Map(routines.map((row, position) => [row.spanId, position]));
  const pairs: [number, number][] = [];
  const edgeRows: { calls: bigint; wall: bigint }[] = [];
  for (const row of routines) {
    for (const caller of row.callers) {
      const from = node.get(caller.spanId);
      const to = node.get(row.spanId);
      if (from === undefined || to === undefined) continue;
      pairs.push([from, to]);
      edgeRows.push({ calls: caller.calls, wall: caller.inclusiveWallNs });
    }
  }
  const layout = layeredLayout(routines.length, pairs);
  const builder = new SceneBuilder({ nodes: routines.length, edges: pairs.length });
  routines.forEach((_, position) => builder.node(layout.x[position], layout.y[position], NODE.function));
  pairs.forEach(([from, to]) => builder.edge(from, to, EDGE.call));
  return finish(
    builder,
    routines.map(() => NODE.function),
    routines.map(() => -1),
    routines.map(row => logSize(row.calls, 6, 20, 3.2)),
    pairs.map(() => EDGE.call),
    edgeRows.map(row => logSize(row.calls, 1.2, 6, 0.9)),
    {
      entities: routines.map(row => ({ kind: 'span', id: row.spanId })),
      labels: routines.map(row => spanShortName(index, row.spanId)),
      priority: Float32Array.from(routines, (_, rank) => (rank < 12 ? 2 : rank < 40 ? 1 : 0)),
      nodeTips: routines.map(row =>
        tooltipCard(spanName(index, row.spanId), [
          ['Calls', formatInteger(row.calls)],
          ['Self active', formatNs(row.selfActiveNs)],
          ['Wall sum', formatNs(row.inclusiveWallNs)],
        ]),
      ),
      edgeTips: pairs.map(([from, to], edge) =>
        tooltipCard(`${spanShortName(index, routines[from].spanId)} → ${spanShortName(index, routines[to].spanId)}`, [
          ['Calls', formatInteger(edgeRows[edge].calls)],
          ['Child wall sum', formatNs(edgeRows[edge].wall)],
        ]),
      ),
    },
  );
}

/** Instances in containment trees, with the functions that touched them on an outer ring. */
export function objectModel(index: ProfileIndex): GraphModel | null {
  const objects = index.objects;
  if (!objects || objects.instances.size === 0) return null;
  const instanceIds = [...objects.instances.keys()].sort((a, b) => a - b);
  const position = new Map(instanceIds.map((id, at) => [id, at]));
  const spanIds = [...objects.instancesBySpan.keys()].sort((a, b) => a - b);
  const typeSlot = new Map(objects.typeSummaries.map((summary, at) => [summary.typeId, at % 8]));
  const parent = Int32Array.from(instanceIds, id => {
    const owner = objects.instances.get(id)!.parent;
    return owner === null || owner === undefined ? -1 : (position.get(asNumber(owner)) ?? -1);
  });
  const group = Int32Array.from(instanceIds, id => asNumber(objects.instances.get(id)!.type_id));
  const links: [number, number][] = [];
  spanIds.forEach((spanId, satellite) => {
    for (const id of objects.instancesBySpan.get(spanId) ?? []) links.push([satellite, position.get(id)!]);
  });
  const count = instanceIds.length;
  // Small graphs read best in layers (functions → instances → nested instances);
  // large forests stay compact on a radial tree.
  const layout =
    count + spanIds.length <= LAYERED_LIMIT
      ? layeredLayout(count + spanIds.length, [
          ...links.map(([satellite, node]) => [count + satellite, node] as const),
          ...[...parent].flatMap((owner, node) => (owner >= 0 ? [[owner, node] as const] : [])),
        ])
      : radialLayout(parent, group, spanIds.length, links);
  const builder = new SceneBuilder({ nodes: count + spanIds.length });
  for (let at = 0; at < count + spanIds.length; at += 1) {
    builder.node(layout.x[at], layout.y[at], at < count ? NODE.instance : NODE.function);
  }
  const edgeKind: number[] = [];
  const edgeWidth: number[] = [];
  const edgeTips: string[] = [];
  const add = (from: number, to: number, kind: number, width: number, tip: string) => {
    builder.edge(from, to, kind);
    edgeKind.push(kind);
    edgeWidth.push(width);
    edgeTips.push(tip);
  };
  instanceIds.forEach((id, at) => {
    const instance = objects.instances.get(id)!;
    const name = instanceName(index, instance);
    if (parent[at] >= 0) add(parent[at], at, EDGE.contain, 1, tooltipCard('Contains', [['Child', name]]));
    if (instance.cloned_from !== null && instance.cloned_from !== undefined) {
      const from = position.get(asNumber(instance.cloned_from));
      if (from !== undefined) add(from, at, EDGE.clone, 1.4, tooltipCard('Cloned into', [['Clone', name]]));
    }
    if (instance.replaced_by !== null && instance.replaced_by !== undefined) {
      const to = position.get(asNumber(instance.replaced_by));
      if (to !== undefined) add(at, to, EDGE.replaced, 1.4, tooltipCard('Replaced by', [['Instance', name]]));
    }
  });
  spanIds.forEach((spanId, satellite) => {
    for (const id of objects.instancesBySpan.get(spanId) ?? []) {
      const calls = objects.instances.get(id)!.calls.find(entry => asNumber(entry.span) === spanId);
      if (!calls) continue;
      const changed = asBigInt(calls.changed_calls) > 0n;
      add(
        count + satellite,
        position.get(id)!,
        changed ? EDGE.changed : EDGE.touch,
        logSize(asBigInt(calls.calls), 1, 4, 0.8),
        tooltipCard(`${spanShortName(index, spanId)} → ${instanceName(index, objects.instances.get(id)!)}`, [
          ['Calls', formatInteger(calls.calls)],
          ['Changed it', formatInteger(calls.changed_calls)],
          ['Touches', formatInteger(calls.touches)],
        ]),
      );
    }
  });
  const instances = instanceIds.map(id => objects.instances.get(id)!);
  return finish(
    builder,
    [...instanceIds.map(() => NODE.instance), ...spanIds.map(() => NODE.function)],
    [...instances.map(instance => typeSlot.get(asNumber(instance.type_id)) ?? 0), ...spanIds.map(() => -1)],
    [...instances.map(instance => logSize(asBigInt(instance.versions), 4.5, 15, 3)), ...spanIds.map(() => 9)],
    edgeKind,
    edgeWidth,
    {
      entities: [
        ...instanceIds.map(id => ({ kind: 'instance', id }) as Entity),
        ...spanIds.map(id => ({ kind: 'span', id }) as Entity),
      ],
      labels: [...instances.map(instance => instanceName(index, instance)), ...spanIds.map(id => spanShortName(index, id))],
      priority: Float32Array.from(
        [...instances.map(instance => (asBigInt(instance.versions) > 0n ? 1 : 0)), ...spanIds.map(() => 2)],
      ),
      nodeTips: [
        ...instances.map(instance =>
          tooltipCard(
            instanceName(index, instance),
            [
              ['Versions', formatInteger(instance.versions)],
              ['Functions', String(instance.calls.length)],
              ['Key', instance.key_kind],
            ],
            shortName(index.objects!.types.get(asNumber(instance.type_id))?.name ?? ''),
          ),
        ),
        ...spanIds.map(id => tooltipCard(spanName(index, id), [['Instances touched', String(objects.instancesBySpan.get(id)?.length ?? 0)]])),
      ],
      edgeTips,
    },
  );
}

export function graphStyles(colors: ThemeColors): SceneStyles {
  const edge = (color: string, width: number, opacity: number, extra: { dashed?: boolean; arrow?: number } = {}) => ({
    color,
    width,
    opacity,
    ...extra,
  });
  const base = [
    edge(colors.muted, 1.4, 0.55, { arrow: 7 }),
    edge(colors.muted, 1, 0.35),
    edge(colors.info, 1.4, 0.8, { dashed: true, arrow: 7 }),
    edge(colors.warning, 1.4, 0.8, { arrow: 7 }),
    edge(colors.muted, 1, 0.25),
    edge(colors.warning, 1.2, 0.55),
  ];
  return {
    nodes: [
      { radius: 8, fill: colors.accent, stroke: colors.surface, strokeWidth: 1.5 },
      { radius: 6, fill: colors.info, stroke: colors.surface, strokeWidth: 1 },
      { radius: 9, fill: colors.highlight, stroke: colors.text, strokeWidth: 2.5 },
      { radius: 7, fill: colors.accent, stroke: colors.highlight, strokeWidth: 2 },
      { radius: 5, fill: colors.dim },
    ],
    edges: [
      ...base,
      ...base.map(style => ({ ...style, color: style.color === colors.muted ? colors.highlight : style.color, opacity: 1 })),
      edge(colors.dim, 1, 0.35),
    ],
  };
}

/**
 * Rewrite style bytes and per-node colors for a selection, without touching
 * geometry; call `layer.restyle()` afterwards.
 */
export function applySelection(model: GraphModel, selected: ReadonlySet<number>, colors: ThemeColors) {
  const { scene } = model;
  const palette = colors.palette.map(packColor);
  const accent = packColor(colors.accent);
  const near = new Set<number>();
  for (const node of selected) for (const other of model.neighbors[node] ?? []) near.add(other);
  const active = selected.size > 0;
  for (let node = 0; node < scene.nodeCount; node += 1) {
    const state = selected.has(node) ? NODE.selected : near.has(node) ? NODE.neighbor : active ? NODE.dim : model.kind[node];
    scene.nodeStyle[node] = state;
    const own = model.slot[node] < 0 ? accent : palette[model.slot[node]];
    scene.nodeColor[node] = state === NODE.selected || state === NODE.dim ? 0 : own;
  }
  for (let edge = 0; edge < scene.edgeCount; edge += 1) {
    const touches = selected.has(scene.edgeFrom[edge]) || selected.has(scene.edgeTo[edge]);
    scene.edgeStyle[edge] = touches ? model.edgeKind[edge] + HIGHLIGHT : active ? DIM_EDGE : model.edgeKind[edge];
  }
}

/** Visibility mask keeping the selection and its direct neighbors. */
export function neighborhoodMask(model: GraphModel, selected: ReadonlySet<number>): Uint8Array {
  const mask = new Uint8Array(model.scene.nodeCount);
  for (const node of selected) {
    mask[node] = 1;
    for (const other of model.neighbors[node] ?? []) mask[other] = 1;
  }
  return mask;
}
