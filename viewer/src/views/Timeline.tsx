import { useMemo } from 'react';
import { asNumber, buildCriticalPath, chainName, formatInteger, formatNs, type ExactRoot, type ProfileIndex } from '../model';
import { useProfile } from '../store';
import { Empty, Note, Panel, RunPicker, StatCard } from '../ui/components';

export function Timeline({ index }: { index: ProfileIndex }) {
  const rootUid = useProfile(state => state.rootUid);
  const chainId = useProfile(state => state.chainId);
  const navigate = useProfile(state => state.navigate);
  const root: ExactRoot | null = (rootUid ? index.roots.get(rootUid) : null) ?? index.profile.roots[index.profile.roots.length - 1] ?? null;
  const critical = useMemo(() => (root ? buildCriticalPath(root) : null), [root]);
  const invocations = useMemo(() => new Map(root?.execution.invocations.map(item => [asNumber(item.id), item]) ?? []), [root]);
  if (!root) return <Empty>No retained root runs are available for a timeline.</Empty>;
  const rootStart = asNumber(root.t_start_ns);
  const duration = Math.max(1, asNumber(root.duration_ns));
  const threads = [...new Set(root.execution.invocations.map(item => asNumber(item.thread)))].sort((a, b) => a - b);
  const criticalSegments = new Set(critical?.segmentIds ?? []);
  const segmentChain = (invocation: number | bigint) => invocations.get(asNumber(invocation))?.chain;
  const routine = (invocation: number | bigint) => {
    const chain = segmentChain(invocation);
    return chain === undefined ? `Invocation #${invocation}` : chainName(index, asNumber(chain));
  };
  return (
    <div className="panel-stack">
      <Panel eyebrow="RUN SELECTOR" title="Choose a retained invocation" aside={`${index.profile.roots.length} runs`}>
        <RunPicker index={index} selected={root} />
      </Panel>
      <div className="stats-grid">
        <StatCard label="Run duration" value={formatNs(root.duration_ns)} detail={root.completion} accent />
        <StatCard label="Retained intervals" value={formatInteger(root.execution.invocations.length)} detail={`Evidence is ${root.execution.status}`} />
        <StatCard label="Participating threads" value={threads.length} detail={`Origin thread #${formatInteger(root.thread)}`} />
        <StatCard
          label="Observed critical path"
          value={critical ? formatNs(critical.durationNs) : 'Unavailable'}
          detail={critical ? `${critical.segmentIds.length} causal segments` : 'Requires explicit segment dependencies'}
        />
      </div>
      <Panel eyebrow="THREAD SWIMLANES" title="Invocation timeline" aside="Click an interval to select its chain · critical segments are highlighted" className="timeline-panel">
        <div className="timeline-axis">
          <span>0</span>
          <span>{formatNs(duration / 2)}</span>
          <span>{formatNs(duration)}</span>
        </div>
        <div className="timeline-lanes">
          {threads.map(thread => (
            <div className="timeline-lane" key={thread}>
              <span className="timeline-thread">Thread #{thread}</span>
              <span className="timeline-track">
                {root.execution.invocations
                  .filter(item => asNumber(item.thread) === thread)
                  .map(item => {
                    const left = Math.max(0, ((asNumber(item.t_start_ns) - rootStart) / duration) * 100);
                    const width = Math.max(0.25, ((asNumber(item.t_end_ns) - asNumber(item.t_start_ns)) / duration) * 100);
                    const chain = asNumber(item.chain);
                    const name = chainName(index, chain);
                    return (
                      <button
                        key={asNumber(item.id)}
                        className={`timeline-interval ${chainId === chain ? 'selected' : ''}`}
                        style={{ left: `${Math.min(100, left)}%`, width: `${Math.min(100 - left, width)}%` }}
                        title={`${name} · ${formatNs(asNumber(item.t_end_ns) - asNumber(item.t_start_ns))}`}
                        onClick={() => navigate({ kind: 'chain', id: chain }, 'stay')}
                      >
                        <span>{name.split('::').pop()}</span>
                      </button>
                    );
                  })}
                {root.execution.segments
                  .filter(item => asNumber(item.thread) === thread && criticalSegments.has(asNumber(item.id)))
                  .map(segment => (
                    <i
                      key={asNumber(segment.id)}
                      className="critical-segment"
                      style={{
                        left: `${Math.max(0, ((asNumber(segment.t_start_ns) - rootStart) / duration) * 100)}%`,
                        width: `${Math.max(0.35, ((asNumber(segment.t_end_ns) - asNumber(segment.t_start_ns)) / duration) * 100)}%`,
                      }}
                      title={`Critical: ${routine(segment.invocation)}`}
                    />
                  ))}
              </span>
            </div>
          ))}
        </div>
        {chainId !== null && (
          <div className="timeline-selection">
            Selected: <strong>{chainName(index, chainId)}</strong>
            <button className="entity-link" onClick={() => navigate({ kind: 'chain', id: chainId }, 'routines')}>
              Open routine →
            </button>
            <button className="entity-link" onClick={() => navigate({ kind: 'chain', id: chainId }, 'chains')}>
              Open chain →
            </button>
          </div>
        )}
        <Note>
          Nested invocation bars can overlap by design. The critical path is computed only from captured exclusive segments and explicit causal
          dependencies; incomplete evidence is never promoted to an exact claim.
        </Note>
      </Panel>
      <Panel eyebrow="CAUSAL PATH" title="Longest observed dependency path" aside={`${root.execution.dependencies.length} dependencies`}>
        {critical ? (
          <div className="critical-list">
            {critical.segmentIds.map((id, position) => {
              const segment = root.execution.segments.find(item => asNumber(item.id) === id)!;
              const chain = segmentChain(segment.invocation);
              return (
                <div key={id}>
                  <span>{position + 1}</span>
                  {chain === undefined ? (
                    <strong>{routine(segment.invocation)}</strong>
                  ) : (
                    <button className="critical-link" onClick={() => navigate({ kind: 'chain', id: asNumber(chain) }, 'routines')}>
                      {routine(segment.invocation)}
                    </button>
                  )}
                  <small>
                    Thread #{formatInteger(segment.thread)} · {formatNs(asNumber(segment.t_end_ns) - asNumber(segment.t_start_ns))}
                  </small>
                </div>
              );
            })}
          </div>
        ) : (
          <Empty>
            No explicit segment DAG is available for this run. The swimlanes remain useful, but SpanScope will not infer causality from overlapping wall
            intervals.
          </Empty>
        )}
      </Panel>
    </div>
  );
}
