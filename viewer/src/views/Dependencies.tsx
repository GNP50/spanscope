import { useMemo } from 'react';
import { asNumber, byBigIntDesc, chainName, formatInteger, formatNs, rootName, scopeLabel, shortName, spanName, type ProfileIndex } from '../model';
import { summarizeRoutines } from '../model/routines';
import { GraphView } from '../graph/GraphView';
import { dependencyModel, entityKey } from '../graph/models';
import { useProfile } from '../store';
import { Empty, Note, Panel } from '../ui/components';
import { useThemeColors } from '../ui/theme';

export function Dependencies({ index }: { index: ProfileIndex }) {
  const rootUid = useProfile(state => state.rootUid);
  const spanId = useProfile(state => state.spanId);
  const navigate = useProfile(state => state.navigate);
  const colors = useThemeColors();
  const routines = useMemo(() => (rootUid ? summarizeRoutines(index, rootUid) : index.routines), [index, rootUid]);
  const model = useMemo(() => dependencyModel(index, routines), [index, routines]);
  const selected = useMemo(() => {
    const node = spanId === null ? undefined : model.lookup.get(entityKey({ kind: 'span', id: spanId }));
    return new Set(node === undefined ? [] : [node]);
  }, [model, spanId]);
  const edges = useMemo(
    () =>
      routines
        .flatMap(row => row.callers.map(caller => ({ from: caller.spanId, to: row.spanId, calls: caller.calls, inclusiveWallNs: caller.inclusiveWallNs })))
        .sort((a, b) => byBigIntDesc(a.calls, b.calls)),
    [routines],
  );
  const selectedRoot = rootUid ? index.roots.get(rootUid) : null;
  const invocationChain = useMemo(
    () => new Map(selectedRoot?.execution.segments.map(segment => [asNumber(segment.id), segment.invocation]) ?? []),
    [selectedRoot],
  );
  return (
    <div className="panel-stack">
      <Panel
        eyebrow={`CALL DEPENDENCIES · ${scopeLabel(index, rootUid)}`}
        title="Who calls whom"
        aside="Scroll to zoom · drag to pan · click a node to select it"
        className="graph-panel"
      >
        {routines.length ? (
          <GraphView
            model={model}
            selected={selected}
            onSelect={entity => navigate(entity, 'stay')}
            ariaLabel="Interactive call relationship graph"
            className="dependency-graph"
            legend={[
              { label: 'Routine (size: calls)', swatch: colors.accent },
              { label: 'Selected', swatch: colors.highlight },
              { label: 'Call (width: calls)', swatch: colors.muted, shape: 'line' },
            ]}
            toolbar={
              spanId !== null && (
                <button className="graph-button" onClick={() => navigate({ kind: 'span', id: spanId }, 'routines')}>
                  Open {shortName(spanName(index, spanId))} →
                </button>
              )
            }
          />
        ) : (
          <Empty>No call relationships in this scope.</Empty>
        )}
        <Note>
          Layers run from callers (top) to callees. Node size is the observed call count; edge width the calls along that edge. Every routine in scope is
          drawn; select one to highlight its direct neighborhood and use Focus to hide the rest. This is not a causal execution DAG.
        </Note>
      </Panel>
      <Panel eyebrow="DIRECT CALL EDGES" title="Relationship counts and cost" aside={`${edges.length} edges`}>
        <div className="edge-list">
          {edges.map(edge => (
            <button key={`${edge.from}:${edge.to}`} onClick={() => navigate({ kind: 'span', id: edge.to }, 'stay')}>
              <span>
                {spanName(index, edge.from)} <b>→</b> {spanName(index, edge.to)}
              </span>
              <strong>{formatInteger(edge.calls)} calls</strong>
              <span>{formatNs(edge.inclusiveWallNs)} child wall sum</span>
            </button>
          ))}
        </div>
        {edges.length === 0 && <Empty>No direct caller/callee edges were recorded.</Empty>}
      </Panel>
      <Panel
        eyebrow="CAUSAL EVIDENCE"
        title="Segment dependencies"
        aside={selectedRoot ? `${selectedRoot.execution.status} · ${selectedRoot.execution.dependencies.length} edges` : 'Select a run above'}
      >
        {!selectedRoot ? (
          <div className="causal-run-list">
            {index.profile.roots.slice(0, 12).map(root => (
              <button key={root.uid} onClick={() => navigate({ kind: 'root', uid: root.uid }, 'stay')}>
                {shortName(rootName(index, root))} · {root.uid.slice(0, 12)} <strong>{root.execution.dependencies.length} causal edges</strong>
              </button>
            ))}
          </div>
        ) : selectedRoot.execution.dependencies.length ? (
          <div className="causal-edge-list">
            {selectedRoot.execution.dependencies.map((edge, position) => {
              const describe = (segment: number | bigint) => {
                const invocation = invocationChain.get(asNumber(segment));
                const chain = selectedRoot.execution.invocations.find(item => item.id === invocation)?.chain;
                return chain === undefined ? `segment #${formatInteger(segment)}` : `${shortName(chainName(index, asNumber(chain)))} (#${formatInteger(segment)})`;
              };
              return (
                <button key={position} onClick={() => navigate({ kind: 'root', uid: selectedRoot.uid }, 'timeline')}>
                  <strong>{edge.kind}</strong>
                  <span>
                    {describe(edge.from)} → {describe(edge.to)}
                  </span>
                </button>
              );
            })}
          </div>
        ) : (
          <Empty>
            This run has no explicit segment dependencies. Its evidence is <strong>{selectedRoot.execution.status}</strong>, so an exact critical path
            cannot be inferred. The caller graph above remains useful for call frequency and aggregate cost.
          </Empty>
        )}
      </Panel>
    </div>
  );
}
