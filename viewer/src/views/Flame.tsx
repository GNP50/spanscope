import { useEffect, useMemo, useRef, useState } from 'react';
import { buildFlame, formatBytes, formatInteger, formatNs, hasHeapData, spanName, type ExactChain, type FlameMetric, type FlameNode, type ProfileIndex } from '../model';
import { useProfile } from '../store';
import { Note, Panel, SourceLocation, ViewLinks } from '../ui/components';
import { useThemeColors } from '../ui/theme';

interface FlameRect {
  x: number;
  y: number;
  width: number;
  height: number;
  node: FlameNode;
}

const ROW = 36;

export function Flame({ index, chains }: { index: ProfileIndex; chains: ExactChain[] }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const layout = useRef<FlameRect[]>([]);
  const [hover, setHover] = useState<{ rect: FlameRect; x: number; y: number } | null>(null);
  const navigate = useProfile(state => state.navigate);
  const spanId = useProfile(state => state.spanId);
  const colors = useThemeColors();
  const heap = hasHeapData(index.profile);
  const [metric, setMetric] = useState<FlameMetric>('time');
  const active: FlameMetric = heap ? metric : 'time';
  const format = active === 'alloc' ? formatBytes : formatNs;
  const nodes = useMemo(() => buildFlame(chains, active), [chains, active]);
  const children = useMemo(() => {
    const map = new Map<string | null, FlameNode[]>();
    for (const node of nodes) {
      const list = map.get(node.parent);
      if (list) list.push(node);
      else map.set(node.parent, [node]);
    }
    return map;
  }, [nodes]);
  const maxDepth = nodes.reduce((max, node) => Math.max(max, node.depth), 0);

  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    const draw = () => {
      const bounds = element.getBoundingClientRect();
      const ratio = window.devicePixelRatio || 1;
      element.width = Math.max(1, Math.floor(bounds.width * ratio));
      element.height = Math.max(1, Math.floor(bounds.height * ratio));
      const ctx = element.getContext('2d');
      if (!ctx) return;
      ctx.scale(ratio, ratio);
      ctx.clearRect(0, 0, bounds.width, bounds.height);
      const rects: FlameRect[] = [];
      const paint = (parent: string | null, left: number, width: number) => {
        const siblings = children.get(parent) ?? [];
        const total = siblings.reduce((sum, node) => sum + Number(node.weight), 0);
        let x = left;
        for (const node of siblings) {
          const w = total ? (width * Number(node.weight)) / total : 0;
          if (w < 1) {
            x += w;
            continue;
          }
          const rect = { x, y: node.depth * ROW + 14, width: Math.max(0, w - 3), height: 31, node };
          rects.push(rect);
          const selected = spanId === node.spanId;
          ctx.fillStyle = selected ? colors.highlight : colors.palette[(node.spanId * 3 + node.depth) % colors.palette.length];
          ctx.globalAlpha = spanId === null || selected ? 1 : 0.55;
          ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
          ctx.globalAlpha = 1;
          if (rect.width > 50) {
            ctx.fillStyle = colors.ink;
            ctx.font = '12px Inter, system-ui, sans-serif';
            ctx.fillText(spanName(index, node.spanId).split('::').pop() ?? '', x + 9, rect.y + 20, Math.max(0, rect.width - 17));
          }
          paint(node.key, x, w);
          x += w;
        }
      };
      paint(null, 0, bounds.width);
      layout.current = rects;
    };
    draw();
    const observer = new ResizeObserver(draw);
    observer.observe(element);
    return () => observer.disconnect();
  }, [children, index, spanId, colors]);

  const pick = (event: React.MouseEvent<HTMLCanvasElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const x = event.clientX - bounds.left;
    const y = event.clientY - bounds.top;
    const rect = [...layout.current].reverse().find(item => x >= item.x && x <= item.x + item.width && y >= item.y && y <= item.y + item.height);
    return rect ? { rect, x, y } : null;
  };
  const selected = spanId !== null ? index.spans.get(spanId) : null;
  const selectedRoutine = spanId !== null ? index.routines.find(row => row.spanId === spanId) : null;

  return (
    <div className="panel-stack">
      <Panel
        eyebrow="STRUCTURAL VIEW"
        title="Flame graph"
        aside={
          heap ? (
            <div className="metric-switch" role="group" aria-label="Flame width metric">
              <button className={active === 'time' ? 'active' : ''} onClick={() => setMetric('time')}>
                Self time
              </button>
              <button className={active === 'alloc' ? 'active' : ''} onClick={() => setMetric('alloc')}>
                Allocated bytes
              </button>
            </div>
          ) : (
            'Widths use aggregate active self time · click a frame to select it'
          )
        }
        className="flame-panel"
      >
        <div className="flame-scroll">
          <canvas
            ref={canvas}
            style={{ height: `${Math.max(230, (maxDepth + 1) * ROW + 30)}px` }}
            onMouseMove={event => setHover(pick(event))}
            onMouseLeave={() => setHover(null)}
            onClick={event => {
              const hit = pick(event);
              if (hit) navigate({ kind: 'span', id: hit.rect.node.spanId }, 'stay');
            }}
            role="img"
            aria-label="Interactive structural flame graph"
          />
          {hover && (
            <div className="flame-tooltip" style={{ left: Math.min(hover.x + 14, 9999), top: hover.y + 14 }}>
              <strong>{spanName(index, hover.rect.node.spanId)}</strong>
              <span>
                {format(hover.rect.node.self)} self · {format(hover.rect.node.weight)} subtree
              </span>
            </div>
          )}
        </div>
        <Note>This is an aggregate call hierarchy, not an execution timeline. Concurrent wall durations are not additive.</Note>
      </Panel>
      {selected && spanId !== null && (
        <section className="panel selection-strip">
          <div>
            <div className="eyebrow">SELECTED FRAME</div>
            <h3>{selected.name}</h3>
            <SourceLocation file={selected.file} line={selected.line} />
          </div>
          {selectedRoutine && (
            <div className="strip-stats">
              <span>
                <small>Calls</small>
                <strong>{formatInteger(selectedRoutine.calls)}</strong>
              </span>
              <span>
                <small>Self active</small>
                <strong>{formatNs(selectedRoutine.selfActiveNs)}</strong>
              </span>
            </div>
          )}
          <ViewLinks
            entity={{ kind: 'span', id: spanId }}
            views={[
              { view: 'routines', label: 'Routine' },
              { view: 'chains', label: 'Chains' },
              { view: 'dependencies', label: 'Graph' },
              { view: 'raw', label: 'Raw' },
            ]}
          />
        </section>
      )}
    </div>
  );
}
