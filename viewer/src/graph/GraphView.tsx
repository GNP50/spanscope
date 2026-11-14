import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { initWasm } from 'skeinmap';
import { GraphCanvas, type GraphCanvasHandle } from 'skeinmap/react';
import 'skeinmap/style.css';
import { useThemeColors } from '../ui/theme';
import type { Entity } from '../nav/views';
import { applySelection, graphStyles, neighborhoodMask, type GraphModel } from './models';

// The WebAssembly kernels are optional; the TypeScript kernels give identical output.
void initWasm();

export interface LegendItem {
  label: string;
  swatch: string;
  shape?: 'dot' | 'line' | 'dash';
}

/**
 * WebGL2 graph (Canvas 2D fallback) with selection highlighting, neighborhood
 * focus, tooltips, and labels. Selection changes only rewrite style bytes.
 */
export function GraphView({
  model,
  selected,
  onSelect,
  ariaLabel,
  legend,
  className = '',
  toolbar,
}: {
  model: GraphModel;
  selected: ReadonlySet<number>;
  onSelect: (entity: Entity) => void;
  ariaLabel: string;
  legend: LegendItem[];
  className?: string;
  toolbar?: ReactNode;
}) {
  const handle = useRef<GraphCanvasHandle>(null);
  const colors = useThemeColors();
  const styles = useMemo(() => graphStyles(colors), [colors]);
  const [focus, setFocus] = useState(false);
  const [backend, setBackend] = useState<string>('');

  // The layer is created by GraphCanvas after mount; wait for it frame by frame.
  const [ready, setReady] = useState(0);
  useEffect(() => {
    let frame = 0;
    let attempts = 0;
    const poll = () => {
      const layer = handle.current?.layer;
      if (layer && layer.scene === model.scene) {
        handle.current?.fit(40);
        setBackend(layer.stats.backend === 'webgl' ? 'WebGL2' : 'Canvas 2D');
        setReady(value => value + 1);
      } else if (attempts++ < 120) frame = requestAnimationFrame(poll);
    };
    frame = requestAnimationFrame(poll);
    return () => cancelAnimationFrame(frame);
  }, [model]);

  useEffect(() => {
    applySelection(model, selected, colors);
    const layer = handle.current?.layer;
    if (!layer) return;
    layer.restyle(styles);
    layer.setFilter(focus && selected.size ? { nodes: neighborhoodMask(model, selected) } : null);
  }, [model, selected, colors, styles, focus, ready]);

  return (
    <div className={`graph-view ${className}`}>
      <div className="graph-toolbar">
        <button className="graph-button" onClick={() => handle.current?.fit(40)} title="Fit the whole graph">
          ⤢ Fit
        </button>
        <button
          className={`graph-button ${focus ? 'active' : ''}`}
          disabled={selected.size === 0}
          onClick={() => setFocus(value => !value)}
          title="Show only the selection and its direct neighbors"
        >
          ◎ Focus
        </button>
        {toolbar}
        <span className="graph-backend">{backend}</span>
      </div>
      <div className="graph-stage" role="img" aria-label={ariaLabel}>
        <GraphCanvas
          ref={handle}
          scene={model.scene}
          styles={styles}
          className="graph-canvas"
          style={{ width: '100%', height: '100%' }}
          labelFont="11px Inter, system-ui, sans-serif"
          overlayColors={{ text: colors.text, halo: colors.surface, accent: colors.highlight }}
          tooltipHtml
          nodeTooltip={node => model.nodeTips[node] ?? null}
          edgeTooltip={edge => model.edgeTips[edge] ?? null}
          nodeLabel={node => model.labels[node] ?? null}
          labelPriority={node => model.priority[node] ?? 0}
          onNodeClick={node => {
            const entity = model.entities[node];
            if (entity) onSelect(entity);
          }}
        />
      </div>
      <div className="graph-legend">
        {legend.map(item => (
          <span key={item.label}>
            <i className={`legend-${item.shape ?? 'dot'}`} style={{ background: item.swatch, borderColor: item.swatch }} />
            {item.label}
          </span>
        ))}
      </div>
    </div>
  );
}
