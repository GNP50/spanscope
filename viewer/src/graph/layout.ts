/**
 * Deterministic graph layouts. skeinmap draws but does not lay out, so positions
 * are computed here: a layered layout for call graphs and a radial tree for
 * object containment. Both are pure and run in O((V + E) · sweeps).
 */

export interface Positions {
  x: Float64Array;
  y: Float64Array;
}

export interface LayeredLayout extends Positions {
  layer: Int32Array;
  /** Edges ignored for layering because they close a cycle. */
  reversed: Set<number>;
}

export interface LayeredOptions {
  layerGap?: number;
  nodeGap?: number;
  sweeps?: number;
}

/**
 * Sugiyama-style layout: depth-first cycle breaking, longest-path layering,
 * barycenter ordering sweeps, then even spacing within each layer. Callers sit
 * above callees (screen y grows downward).
 */
export function layeredLayout(count: number, edges: readonly (readonly [number, number])[], options: LayeredOptions = {}): LayeredLayout {
  const { layerGap = 130, nodeGap = 90, sweeps = 4 } = options;
  const out: number[][] = Array.from({ length: count }, () => []);
  edges.forEach(([from, to], edge) => {
    if (from !== to && from >= 0 && to >= 0 && from < count && to < count) out[from].push(edge);
  });
  // Iterative DFS; an edge into a node still on the stack closes a cycle.
  const state = new Uint8Array(count); // 0 new, 1 on stack, 2 done
  const reversed = new Set<number>();
  for (let start = 0; start < count; start += 1) {
    if (state[start]) continue;
    const stack: [number, number][] = [[start, 0]];
    state[start] = 1;
    while (stack.length) {
      const frame = stack[stack.length - 1];
      const [node, next] = frame;
      if (next >= out[node].length) {
        state[node] = 2;
        stack.pop();
        continue;
      }
      frame[1] += 1;
      const edge = out[node][next];
      const target = edges[edge][1];
      if (state[target] === 1) reversed.add(edge);
      else if (state[target] === 0) {
        state[target] = 1;
        stack.push([target, 0]);
      }
    }
  }
  // Longest-path layering over the acyclic edges (Kahn order).
  const indegree = new Int32Array(count);
  const forward: number[][] = Array.from({ length: count }, () => []);
  const backward: number[][] = Array.from({ length: count }, () => []);
  out.forEach((list, from) => {
    for (const edge of list) {
      if (reversed.has(edge)) continue;
      const to = edges[edge][1];
      forward[from].push(to);
      backward[to].push(from);
      indegree[to] += 1;
    }
  });
  const layer = new Int32Array(count);
  const queue: number[] = [];
  for (let node = 0; node < count; node += 1) if (indegree[node] === 0) queue.push(node);
  for (let head = 0; head < queue.length; head += 1) {
    const node = queue[head];
    for (const to of forward[node]) {
      layer[to] = Math.max(layer[to], layer[node] + 1);
      indegree[to] -= 1;
      if (indegree[to] === 0) queue.push(to);
    }
  }
  const depth = count ? layer.reduce((max, value) => Math.max(max, value), 0) + 1 : 0;
  const layers: number[][] = Array.from({ length: depth }, () => []);
  for (let node = 0; node < count; node += 1) layers[layer[node]].push(node);
  const position = new Float64Array(count);
  const place = (list: number[]) => list.forEach((node, at) => (position[node] = at));
  layers.forEach(place);
  const reorder = (list: number[], neighbors: number[][]) => {
    const key = new Map<number, number>();
    for (const node of list) {
      const adjacent = neighbors[node];
      key.set(node, adjacent.length ? adjacent.reduce((sum, other) => sum + position[other], 0) / adjacent.length : position[node]);
    }
    list.sort((a, b) => key.get(a)! - key.get(b)! || position[a] - position[b]);
    place(list);
  };
  for (let sweep = 0; sweep < sweeps; sweep += 1) {
    for (let at = 1; at < depth; at += 1) reorder(layers[at], backward);
    for (let at = depth - 2; at >= 0; at -= 1) reorder(layers[at], forward);
  }
  const x = new Float64Array(count);
  const y = new Float64Array(count);
  layers.forEach((list, at) => {
    list.forEach((node, slot) => {
      x[node] = (slot - (list.length - 1) / 2) * nodeGap;
      y[node] = at * layerGap;
    });
  });
  return { x, y, layer, reversed };
}

export interface RadialOptions {
  ringGap?: number;
  nodeGap?: number;
}

/**
 * Radial tree for a containment forest plus satellites on an outer ring.
 * `parent[i]` is the containing node or -1; roots are ordered by `group` so
 * instances of one type sit together. Satellites (functions) are placed at the
 * circular mean angle of the nodes they link to, then spread to avoid overlap.
 * Returns positions for the tree nodes followed by the satellites.
 */
export function radialLayout(
  parent: Int32Array,
  group: Int32Array,
  satellites: number,
  links: readonly (readonly [number, number])[],
  options: RadialOptions = {},
): Positions {
  const { ringGap = 95, nodeGap = 30 } = options;
  const count = parent.length;
  const total = count + satellites;
  const x = new Float64Array(total);
  const y = new Float64Array(total);
  if (total === 0) return { x, y };
  const children: number[][] = Array.from({ length: count }, () => []);
  for (let node = 0; node < count; node += 1) {
    const owner = parent[node];
    if (owner >= 0 && owner < count && owner !== node) children[owner].push(node);
  }
  // Roots: nodes without a valid parent, then anything only reachable through a cycle.
  const reached = new Uint8Array(count);
  const order: number[] = [];
  const depth = new Int32Array(count);
  const visit = (root: number) => {
    const stack = [root];
    reached[root] = 1;
    while (stack.length) {
      const node = stack.pop()!;
      order.push(node);
      for (const child of children[node]) {
        if (reached[child]) continue;
        reached[child] = 1;
        depth[child] = depth[node] + 1;
        stack.push(child);
      }
    }
  };
  const roots: number[] = [];
  for (let node = 0; node < count; node += 1) {
    const owner = parent[node];
    if (owner < 0 || owner >= count || owner === node) roots.push(node);
  }
  roots.sort((a, b) => group[a] - group[b] || a - b);
  for (const root of roots) visit(root);
  for (let node = 0; node < count; node += 1) {
    if (!reached[node]) {
      roots.push(node);
      visit(node);
    }
  }
  // Leaf counts give each subtree an angular share proportional to its size.
  const leaves = new Float64Array(count);
  for (let at = order.length - 1; at >= 0; at -= 1) {
    const node = order[at];
    const kids = children[node].filter(child => depth[child] === depth[node] + 1);
    leaves[node] = kids.length ? kids.reduce((sum, child) => sum + leaves[child], 0) : 1;
  }
  const maxDepth = count ? depth.reduce((max, value) => Math.max(max, value), 0) : -1;
  const perDepth = new Int32Array(maxDepth + 1);
  for (let node = 0; node < count; node += 1) perDepth[depth[node]] += 1;
  const radius = new Float64Array(maxDepth + 1);
  for (let level = 0; level <= maxDepth; level += 1) {
    const spread = (perDepth[level] * nodeGap) / (2 * Math.PI);
    radius[level] = Math.max((level + 1) * ringGap, spread, level ? radius[level - 1] + ringGap : 0);
  }
  if (count === 1 && satellites === 0) return { x, y };
  const angle = new Float64Array(count);
  const allLeaves = roots.reduce((sum, root) => sum + leaves[root], 0);
  const assign = (node: number, start: number, span: number) => {
    angle[node] = start + span / 2;
    const r = radius[depth[node]];
    x[node] = Math.cos(angle[node]) * r;
    y[node] = Math.sin(angle[node]) * r;
    let cursor = start;
    for (const child of children[node]) {
      if (depth[child] !== depth[node] + 1) continue;
      const share = (span * leaves[child]) / leaves[node];
      assign(child, cursor, share);
      cursor += share;
    }
  };
  let cursor = -Math.PI / 2;
  for (const root of roots) {
    const span = (2 * Math.PI * leaves[root]) / allLeaves;
    assign(root, cursor, span);
    cursor += span;
  }
  if (satellites) {
    const sumX = new Float64Array(satellites);
    const sumY = new Float64Array(satellites);
    for (const [satellite, node] of links) {
      if (satellite < 0 || satellite >= satellites || node < 0 || node >= count) continue;
      sumX[satellite] += Math.cos(angle[node]);
      sumY[satellite] += Math.sin(angle[node]);
    }
    const wanted = Array.from({ length: satellites }, (_, i) =>
      sumX[i] || sumY[i] ? Math.atan2(sumY[i], sumX[i]) : -Math.PI / 2 + (2 * Math.PI * i) / satellites,
    );
    const ordered = wanted.map((value, i) => [value, i] as const).sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    const outer = Math.max(
      (maxDepth >= 0 ? radius[maxDepth] : 0) + 2 * ringGap,
      (satellites * nodeGap * 2.2) / (2 * Math.PI),
    );
    const gap = (nodeGap * 2.2) / outer;
    let previous = -Infinity;
    for (const [value, satellite] of ordered) {
      const placed = Math.max(value, previous + gap);
      previous = placed;
      x[count + satellite] = Math.cos(placed) * outer;
      y[count + satellite] = Math.sin(placed) * outer;
    }
  }
  return { x, y };
}
