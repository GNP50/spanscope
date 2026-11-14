import { useMemo } from 'react';
import {
  asBigInt,
  asNumber,
  formatInteger,
  formatNs,
  maxBigInt,
  rootName,
  scopeLabel,
  shortName,
  spanName,
  spanShortName,
  type ProfileIndex,
} from '../model';
import { routineByRoot, summarizeRoutines, type CallRelation, type RoutineSummary } from '../model/routines';
import { useProfile } from '../store';
import { Empty, EntityLink, Note, Panel, SourceLocation, ViewLinks, WeightBar } from '../ui/components';

function RelationList({ title, empty, relations, index }: { title: string; empty: string; relations: CallRelation[]; index: ProfileIndex }) {
  const navigate = useProfile(state => state.navigate);
  const max = maxBigInt(relations.map(row => row.calls));
  return (
    <Panel eyebrow="DIRECT RELATIONSHIPS" title={title} aside={`${relations.length} routines`} className="relation-panel">
      {relations.length === 0 ? (
        <p className="analysis-empty">{empty}</p>
      ) : (
        <div className="relation-list">
          {relations.map(row => (
            <button key={row.spanId} className="relation-row" onClick={() => navigate({ kind: 'span', id: row.spanId }, 'stay')}>
              <span className="relation-main">
                <strong title={spanName(index, row.spanId)}>{spanName(index, row.spanId)}</strong>
                <WeightBar value={row.calls} max={max} />
              </span>
              <span className="relation-number">
                <strong>{formatInteger(row.calls)}</strong>
                <small>calls</small>
              </span>
              <span className="relation-number">
                <strong>{formatNs(row.inclusiveWallNs)}</strong>
                <small>child wall sum</small>
              </span>
            </button>
          ))}
        </div>
      )}
    </Panel>
  );
}

/** Fields of tracked objects this routine read, wrote, or changed. */
function ObjectsTouched({ index, spanId }: { index: ProfileIndex; spanId: number }) {
  const objects = index.objects;
  const fields = objects?.fieldsBySpan.get(spanId) ?? [];
  const instances = objects?.instancesBySpan.get(spanId) ?? [];
  if (!objects || (fields.length === 0 && instances.length === 0)) return null;
  const byType = new Map<number, typeof fields>();
  for (const field of fields) {
    const typeId = asNumber(field.type_id);
    byType.set(typeId, [...(byType.get(typeId) ?? []), field]);
  }
  return (
    <Panel eyebrow="TRACKED OBJECTS" title="Objects touched" aside={`${instances.length} instances`}>
      <div className="touched-types">
        {[...byType].map(([typeId, rows]) => (
          <div key={typeId} className="touched-type">
            <EntityLink entity={{ kind: 'type', id: typeId }} />
            <div className="field-mini">
              <span>Path</span>
              <span>Reads</span>
              <span>Writes</span>
              <span>Calls</span>
              <span>Changes</span>
              {rows
                .sort((a, b) => a.path.localeCompare(b.path))
                .map(row => (
                  <FieldCells key={row.path} path={row.path} reads={row.reads} writes={row.writes} calls={row.calls} changes={row.changes} />
                ))}
            </div>
          </div>
        ))}
      </div>
      {instances.length > 0 && (
        <div className="instance-chips">
          {instances.slice(0, 24).map(id => (
            <EntityLink key={id} entity={{ kind: 'instance', id }} />
          ))}
          {instances.length > 24 && <span className="subtle">+{instances.length - 24} more in Objects</span>}
        </div>
      )}
    </Panel>
  );
}

function FieldCells(props: { path: string; reads: number | bigint; writes: number | bigint; calls: number | bigint; changes: number | bigint }) {
  const wasted = asBigInt(props.writes) > 0n && asBigInt(props.changes) === 0n;
  return (
    <>
      <code>{props.path || '(whole value)'}</code>
      <span>{formatInteger(props.reads)}</span>
      <span className={wasted ? 'warn' : ''} title={wasted ? 'Written but never changed' : undefined}>
        {formatInteger(props.writes)}
      </span>
      <span>{formatInteger(props.calls)}</span>
      <strong>{formatInteger(props.changes)}</strong>
    </>
  );
}

function RoutineDetail({ index, selected, rootUid }: { index: ProfileIndex; selected: RoutineSummary; rootUid: string | null }) {
  const navigate = useProfile(state => state.navigate);
  const span = index.spans.get(selected.spanId);
  const runRows = useMemo(() => (rootUid ? [] : routineByRoot(index, selected.spanId)), [index, selected.spanId, rootUid]);
  return (
    <div className="routine-detail">
      <section className="panel detail-hero">
        <div className="eyebrow">ROUTINE BREAKDOWN · {scopeLabel(index, rootUid)}</div>
        <h2>{spanName(index, selected.spanId)}</h2>
        <div className="source-line">
          <SourceLocation file={span?.file} line={span?.line} />
          <span>Span #{selected.spanId}</span>
        </div>
        <div className="detail-stats">
          <div>
            <small>Observed calls</small>
            <strong>{formatInteger(selected.calls)}</strong>
          </div>
          <div>
            <small>Exclusive active</small>
            <strong>{formatNs(selected.selfActiveNs)}</strong>
          </div>
          <div>
            <small>Inclusive wall sum</small>
            <strong>{formatNs(selected.inclusiveWallNs)}</strong>
          </div>
          <div>
            <small>Mean wall / call</small>
            <strong>{formatNs(selected.calls ? selected.inclusiveWallNs / selected.calls : 0n)}</strong>
          </div>
        </div>
        <ViewLinks
          entity={{ kind: 'span', id: selected.spanId }}
          views={[
            { view: 'flame', label: 'Flame' },
            { view: 'chains', label: 'Chains' },
            { view: 'dependencies', label: 'Graph' },
            { view: 'raw', label: 'Raw' },
          ]}
        />
        <Note>
          Exclusive active is time spent in this routine outside instrumented children. Inclusive wall includes children and can overlap across concurrent
          work. These totals are observed, not an estimate of unsampled calls.
        </Note>
      </section>
      <div className="relation-grid">
        <RelationList title="Called by" empty="Root entry: no instrumented caller is recorded." relations={selected.callers} index={index} />
        <RelationList title="Calls into" empty="No instrumented child routine was observed." relations={selected.callees} index={index} />
      </div>
      <ObjectsTouched index={index} spanId={selected.spanId} />
      <Panel eyebrow="CALL SITES" title="Full paths to this routine" aside={`${selected.paths.length} paths`}>
        <div className="path-breakdown">
          {selected.paths.map(item => (
            <button
              key={asNumber(item.chain.id)}
              className="path-breakdown-row"
              onClick={() => navigate({ kind: 'chain', id: asNumber(item.chain.id) }, 'chains')}
              title={item.chain.path.map(id => spanName(index, asNumber(id))).join(' → ')}
            >
              <span>{item.chain.path.map(id => spanShortName(index, asNumber(id))).join(' → ')}</span>
              <strong>{formatInteger(item.calls)} calls</strong>
              <span>{formatNs(item.selfActiveNs)} self</span>
            </button>
          ))}
        </div>
      </Panel>
      {!rootUid && (
        <Panel eyebrow="RUN COMPARISON" title="Calls in each retained run" aside="Click a run to inspect it">
          {runRows.length === 0 ? (
            <Empty>No retained run includes this routine. Global aggregates may include evicted roots.</Empty>
          ) : (
            <div className="run-comparison">
              {runRows.map(row => {
                const root = index.roots.get(row.uid)!;
                return (
                  <button key={row.uid} onClick={() => navigate({ kind: 'root', uid: row.uid }, 'runs')}>
                    <span>
                      {shortName(rootName(index, root))}
                      <small>{row.uid.slice(0, 12)}</small>
                    </span>
                    <strong>{formatInteger(row.calls)} calls</strong>
                    <span>{formatNs(row.selfActiveNs)} self</span>
                  </button>
                );
              })}
            </div>
          )}
        </Panel>
      )}
    </div>
  );
}

export function Routines({ index }: { index: ProfileIndex }) {
  const rootUid = useProfile(state => state.rootUid);
  const query = useProfile(state => state.query);
  const selectedId = useProfile(state => state.spanId);
  const navigate = useProfile(state => state.navigate);
  const all = useMemo(() => (rootUid ? summarizeRoutines(index, rootUid) : index.routines), [index, rootUid]);
  const needle = query.trim().toLowerCase();
  const visible = useMemo(() => all.filter(row => spanName(index, row.spanId).toLowerCase().includes(needle)), [all, index, needle]);
  const selected = all.find(row => row.spanId === selectedId) ?? visible[0] ?? all[0];
  const maxSelf = maxBigInt(visible.map(row => row.selfActiveNs));
  return (
    <div className="analysis-layout">
      <Panel eyebrow="ROUTINE COST" title="Every observed routine" aside={scopeLabel(index, rootUid)} className="routine-list-panel">
        <div className="analysis-table-head">
          <span>Routine / self weight</span>
          <span>Calls</span>
          <span>Self active</span>
          <span>Wall sum</span>
        </div>
        <div className="routine-list">
          {visible.map(row => (
            <button
              key={row.spanId}
              className={`routine-row ${selected?.spanId === row.spanId ? 'selected' : ''}`}
              onClick={() => navigate({ kind: 'span', id: row.spanId }, 'stay')}
            >
              <span className="routine-name">
                <strong title={spanName(index, row.spanId)}>{spanName(index, row.spanId)}</strong>
                <WeightBar value={row.selfActiveNs} max={maxSelf} />
              </span>
              <strong>{formatInteger(row.calls)}</strong>
              <span>{formatNs(row.selfActiveNs)}</span>
              <span>{formatNs(row.inclusiveWallNs)}</span>
            </button>
          ))}
        </div>
        {visible.length === 0 && <p className="analysis-empty">No routine matches this search.</p>}
      </Panel>
      {selected && <RoutineDetail index={index} selected={selected} rootUid={rootUid} />}
    </div>
  );
}
