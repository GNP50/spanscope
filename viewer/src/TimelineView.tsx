import { useMemo } from 'react';
import { asNumber, buildCriticalPath, formatInteger, formatNs, type ExactRoot, type ProfileIndex } from './core';
import { useProfile } from './store';

function routine(index: ProfileIndex, root: ExactRoot, invocationId: number): string {
  const invocation = root.execution.invocations.find(item => asNumber(item.id) === invocationId);
  const chain = invocation ? index.chains.get(asNumber(invocation.chain)) : null;
  return chain ? index.spans.get(asNumber(chain.path[chain.path.length - 1]))?.name ?? `Chain #${chain.id}` : `Invocation #${invocationId}`;
}

export function TimelineView({ index, rootUid }: { index: ProfileIndex; rootUid: string | null }) {
  const selectRoot = useProfile(state => state.selectRoot);
  const root = (rootUid ? index.roots.get(rootUid) : null) ?? index.profile.roots[index.profile.roots.length - 1] ?? null;
  const critical = useMemo(() => root ? buildCriticalPath(root) : null, [root]);
  if (!root) return <div className="panel analysis-empty">No retained root runs are available for a timeline.</div>;
  const rootStart = asNumber(root.t_start_ns);
  const duration = Math.max(1, asNumber(root.duration_ns));
  const threads = [...new Set(root.execution.invocations.map(item => asNumber(item.thread)))].sort((a, b) => a - b);
  const criticalSegments = new Set(critical?.segmentIds ?? []);
  return <div className="panel-stack">
    <section className="panel"><div className="panel-heading"><div><div className="eyebrow">RUN SELECTOR</div><h2>Choose a retained invocation</h2></div><span className="subtle">{index.profile.roots.length} runs</span></div><div className="run-picker">{index.profile.roots.map(candidate => <button key={candidate.uid} className={candidate.uid === root.uid ? 'selected' : ''} onClick={() => selectRoot(candidate.uid)}><small>{index.spans.get(asNumber(candidate.span))?.name ?? candidate.span}<br />{candidate.uid}</small><strong>{formatNs(candidate.duration_ns)}</strong></button>)}</div></section>
    <div className="stats-grid"><div className="stat-card accent"><div className="stat-label">Run duration</div><div className="stat-value">{formatNs(root.duration_ns)}</div><div className="stat-detail">{root.completion}</div></div><div className="stat-card"><div className="stat-label">Retained intervals</div><div className="stat-value">{formatInteger(root.execution.invocations.length)}</div><div className="stat-detail">Evidence is {root.execution.status}</div></div><div className="stat-card"><div className="stat-label">Participating threads</div><div className="stat-value">{threads.length}</div><div className="stat-detail">Origin thread #{formatInteger(root.thread)}</div></div><div className="stat-card"><div className="stat-label">Observed critical path</div><div className="stat-value">{critical ? formatNs(critical.durationNs) : 'Unavailable'}</div><div className="stat-detail">{critical ? `${critical.segmentIds.length} causal segments` : 'Requires explicit segment dependencies'}</div></div></div>
    <section className="panel timeline-panel"><div className="panel-heading"><div><div className="eyebrow">THREAD SWIMLANES</div><h2>Invocation timeline</h2></div><span className="subtle">Intervals use wall time · critical segments are highlighted</span></div><div className="timeline-axis"><span>0</span><span>{formatNs(duration / 2)}</span><span>{formatNs(duration)}</span></div><div className="timeline-lanes">{threads.map(thread => <div className="timeline-lane" key={thread}><span className="timeline-thread">Thread #{thread}</span><span className="timeline-track">{root.execution.invocations.filter(item => asNumber(item.thread) === thread).map(item => {
      const left = Math.max(0, (asNumber(item.t_start_ns) - rootStart) / duration * 100);
      const width = Math.max(.25, (asNumber(item.t_end_ns) - asNumber(item.t_start_ns)) / duration * 100);
      const chain = index.chains.get(asNumber(item.chain));
      const name = chain ? index.spans.get(asNumber(chain.path[chain.path.length - 1]))?.name ?? `Chain #${chain.id}` : `Chain #${item.chain}`;
      return <button key={asNumber(item.id)} className="timeline-interval" style={{ left: `${Math.min(100, left)}%`, width: `${Math.min(100 - left, width)}%` }} title={`${name} · ${formatNs(asNumber(item.t_end_ns) - asNumber(item.t_start_ns))}`}><span>{name.split('::').pop()}</span></button>;
    })}{root.execution.segments.filter(item => asNumber(item.thread) === thread && criticalSegments.has(asNumber(item.id))).map(segment => <i key={asNumber(segment.id)} className="critical-segment" style={{ left: `${Math.max(0, (asNumber(segment.t_start_ns) - rootStart) / duration * 100)}%`, width: `${Math.max(.35, (asNumber(segment.t_end_ns) - asNumber(segment.t_start_ns)) / duration * 100)}%` }} title={`Critical: ${routine(index, root, asNumber(segment.invocation))}`} />)}</span></div>)}</div><p className="panel-note">Nested invocation bars can overlap by design. The critical path is computed only from captured exclusive segments and explicit causal dependencies; incomplete evidence is never promoted to an exact claim.</p></section>
    <section className="panel"><div className="panel-heading"><div><div className="eyebrow">CAUSAL PATH</div><h2>Longest observed dependency path</h2></div><span className="subtle">{root.execution.dependencies.length} dependencies</span></div>{critical ? <div className="critical-list">{critical.segmentIds.map((id, position) => { const segment = root.execution.segments.find(item => asNumber(item.id) === id)!; return <div key={id}><span>{position + 1}</span><strong>{routine(index, root, asNumber(segment.invocation))}</strong><small>Thread #{formatInteger(segment.thread)} · {formatNs(asNumber(segment.t_end_ns) - asNumber(segment.t_start_ns))}</small></div>; })}</div> : <div className="analysis-empty">No explicit segment DAG is available for this run. The swimlanes remain useful, but SpanScope will not infer causality from overlapping wall intervals.</div>}</section>
  </div>;
}
