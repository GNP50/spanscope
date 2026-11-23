import { useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  asBigInt,
  asNumber,
  byBigIntDesc,
  formatInteger,
  formatNs,
  formatPercent,
  instanceName,
  maxBigInt,
  shortName,
  spanShortName,
  typeName,
  type ExactInstance,
  type ObjectsIndex,
  type ProfileIndex,
} from '../model';
import { fieldRows, pathDepth } from '../model/objects';
import { GraphView } from '../graph/GraphView';
import { entityKey, objectModel } from '../graph/models';
import { useProfile } from '../store';
import { Empty, EntityLink, Note, Panel, SourceLocation, StatCard, ViewLinks, WeightBar } from '../ui/components';
import { useThemeColors } from '../ui/theme';

const KEY_KIND: Record<string, string> = {
  generated: 'InstanceId',
  domain: 'Domain key',
  address: 'Address (approximate)',
};

function NoObjects() {
  return (
    <section className="panel compare-empty">
      <div className="eyebrow">OBJECT TRACKING</div>
      <h2>No tracked objects in this profile</h2>
      <p>
        Build with the <code>object-tracking</code> feature, derive <code>Tracked</code> on your types, and annotate functions with{' '}
        <code>#[trace(track(arg))]</code>. Each call then records which fields were read or written and whether the value actually changed.
      </p>
    </section>
  );
}

function TypeList({ index, objects }: { index: ProfileIndex; objects: ObjectsIndex }) {
  const typeId = useProfile(state => state.typeId);
  const navigate = useProfile(state => state.navigate);
  const clear = useProfile(state => state.clear);
  const max = maxBigInt(objects.typeSummaries.map(row => row.versions));
  return (
    <Panel eyebrow="TYPES" title="Tracked types" aside={`${objects.types.size}`}>
      <div className="type-list">
        <button className={`type-row ${typeId === null ? 'selected' : ''}`} onClick={() => clear('typeId')}>
          <span>
            <strong>All types</strong>
            <small>{objects.instances.size} instances</small>
          </span>
        </button>
        {objects.typeSummaries.map(row => (
          <button
            key={row.typeId}
            className={`type-row ${typeId === row.typeId ? 'selected' : ''}`}
            onClick={() => navigate({ kind: 'type', id: row.typeId }, 'stay')}
            title={typeName(index, row.typeId)}
          >
            <span>
              <strong>{shortName(typeName(index, row.typeId))}</strong>
              <small>
                {row.instances} instances · {row.changedInstances} changed
                {row.approximate > 0 ? ` · ${row.approximate} approximate` : ''}
              </small>
              <WeightBar value={row.versions} max={max} tone="blue" />
            </span>
            <strong>{formatInteger(row.versions)}</strong>
          </button>
        ))}
      </div>
    </Panel>
  );
}

function InstanceList({ index, objects }: { index: ProfileIndex; objects: ObjectsIndex }) {
  const typeId = useProfile(state => state.typeId);
  const instanceId = useProfile(state => state.instanceId);
  const navigate = useProfile(state => state.navigate);
  const [query, setQuery] = useState('');
  const scroll = useRef<HTMLDivElement>(null);
  const ids = useMemo(() => {
    const pool =
      typeId !== null
        ? (objects.instancesByType.get(typeId) ?? [])
        : [...objects.instances.keys()].sort((a, b) => byBigIntDesc(asBigInt(objects.instances.get(a)!.versions), asBigInt(objects.instances.get(b)!.versions)) || a - b);
    const needle = query.trim().toLowerCase();
    return needle ? pool.filter(id => instanceName(index, objects.instances.get(id)!).toLowerCase().includes(needle)) : pool;
  }, [index, objects, typeId, query]);
  const virtualizer = useVirtualizer({ count: ids.length, getScrollElement: () => scroll.current, estimateSize: () => 46, overscan: 10 });
  return (
    <Panel eyebrow="INSTANCES" title={typeId !== null ? shortName(typeName(index, typeId)) : 'All instances'} aside={`${ids.length} · most versions first`}>
      <div className="invocation-controls">
        <input aria-label="Search instances" placeholder="Search a key…" value={query} onChange={event => setQuery(event.target.value)} />
      </div>
      <div className="instance-scroll" ref={scroll}>
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map(item => {
            const id = ids[item.index];
            const instance = objects.instances.get(id)!;
            return (
              <button
                key={id}
                className={`instance-row ${instanceId === id ? 'selected' : ''}`}
                style={{ transform: `translateY(${item.start}px)` }}
                onClick={() => navigate({ kind: 'instance', id }, 'stay')}
              >
                <span>
                  <strong>{instanceName(index, instance)}</strong>
                  <small>
                    {instance.calls.length} functions{instance.key_kind === 'address' ? ' · approximate' : ''}
                  </small>
                </span>
                <span className="instance-versions">{formatInteger(instance.versions)} v</span>
              </button>
            );
          })}
        </div>
      </div>
      {ids.length === 0 && <Empty>No instance matches.</Empty>}
    </Panel>
  );
}

function Lineage({ objects, instanceId, instance }: { objects: ObjectsIndex; instanceId: number; instance: ExactInstance }) {
  const groups: [string, number[]][] = [
    ['Contained in', instance.parent === null || instance.parent === undefined ? [] : [asNumber(instance.parent)]],
    ['Contains', objects.children.get(instanceId) ?? []],
    ['Cloned from', instance.cloned_from === null || instance.cloned_from === undefined ? [] : [asNumber(instance.cloned_from)]],
    ['Clones', objects.clones.get(instanceId) ?? []],
    ['Replaced by', instance.replaced_by === null || instance.replaced_by === undefined ? [] : [asNumber(instance.replaced_by)]],
    ['Replaces', objects.replaces.get(instanceId) ?? []],
  ];
  const present = groups.filter(([, ids]) => ids.length > 0);
  if (present.length === 0) return null;
  return (
    <div className="lineage">
      {present.map(([title, ids]) => (
        <div key={title}>
          <small>{title}</small>
          <span>
            {ids.slice(0, 30).map(id => (
              <EntityLink key={id} entity={{ kind: 'instance', id }} view="stay" />
            ))}
            {ids.length > 30 && <em>+{ids.length - 30}</em>}
          </span>
        </div>
      ))}
    </div>
  );
}

function InstanceDetail({ index, objects, instanceId }: { index: ProfileIndex; objects: ObjectsIndex; instanceId: number }) {
  const instance = objects.instances.get(instanceId);
  if (!instance) return <Empty>This instance is not in the profile.</Empty>;
  const typeId = asNumber(instance.type_id);
  const fieldChanges = Object.entries(instance.field_changes)
    .map(([path, count]) => ({ path, count: asBigInt(count) }))
    .sort((a, b) => byBigIntDesc(a.count, b.count) || a.path.localeCompare(b.path));
  const maxChange = maxBigInt(fieldChanges.map(row => row.count));
  const calls = [...instance.calls].sort((a, b) => byBigIntDesc(asBigInt(a.calls), asBigInt(b.calls)));
  const history = [...instance.history].reverse();
  return (
    <div className="object-detail">
      <section className="panel detail-hero">
        <div className="eyebrow">
          INSTANCE · {KEY_KIND[instance.key_kind] ?? instance.key_kind}
        </div>
        <h2>{instanceName(index, instance)}</h2>
        <div className="source-line">
          <EntityLink entity={{ kind: 'type', id: typeId }} view="stay" />
          <span>Instance #{instanceId}</span>
        </div>
        <div className="detail-stats">
          <div>
            <small>Versions</small>
            <strong>{formatInteger(instance.versions)}</strong>
          </div>
          <div>
            <small>Functions</small>
            <strong>{instance.calls.length}</strong>
          </div>
          <div>
            <small>Calls received</small>
            <strong>{formatInteger(instance.calls.reduce((sum, row) => sum + asBigInt(row.calls), 0n))}</strong>
          </div>
          <div>
            <small>Changed paths</small>
            <strong>{fieldChanges.length}</strong>
          </div>
        </div>
        <Lineage objects={objects} instanceId={instanceId} instance={instance} />
        <ViewLinks entity={{ kind: 'instance', id: instanceId }} views={[{ view: 'raw', label: 'Raw' }]} />
        {instance.key_kind === 'address' && (
          <Note>This instance has no ID field, so it is keyed by memory address. Moves change the address and freed addresses are reused.</Note>
        )}
      </section>
      <Panel eyebrow="FUNCTIONS" title="Who used this instance" aside="Calls · changed it · executed access sites">
        {calls.length === 0 ? (
          <Empty>Only changed as a nested value; no tracked function received it directly.</Empty>
        ) : (
          <div className="object-table calls-table">
            <span>Function</span>
            <span>Calls</span>
            <span>Changed</span>
            <span>Rate</span>
            <span>Touches</span>
            {calls.map(row => (
              <CallRow key={asNumber(row.span)} row={row} />
            ))}
          </div>
        )}
      </Panel>
      <Panel eyebrow="FIELDS" title="What changed" aside={`${fieldChanges.length} paths`}>
        {fieldChanges.length === 0 ? (
          <Empty>The value never changed in a tracked call.</Empty>
        ) : (
          <div className="field-changes">
            {fieldChanges.map(row => (
              <div key={row.path} style={{ paddingLeft: `${pathDepth(row.path) * 14}px` }}>
                <code>{row.path || '(whole value)'}</code>
                <WeightBar value={row.count} max={maxChange} tone="warm" />
                <strong>{formatInteger(row.count)}</strong>
              </div>
            ))}
          </div>
        )}
      </Panel>
      <Panel
        eyebrow="HISTORY"
        title="Version timeline"
        aside={asNumber(instance.history_dropped) > 0 ? `${history.length} newest · ${formatInteger(instance.history_dropped)} older dropped` : `${history.length} versions`}
      >
        {history.length === 0 ? (
          <Empty>No retained versions.</Empty>
        ) : (
          <ol className="version-list">
            {history.map(version => {
              const span = index.spans.get(asNumber(version.span));
              return (
                <li key={asNumber(version.version)}>
                  <span className="version-badge">v{formatInteger(version.version)}</span>
                  <div>
                    <div className="version-head">
                      <EntityLink entity={{ kind: 'span', id: asNumber(version.span) }} />
                      <small>at +{formatNs(version.t_ns)}</small>
                    </div>
                    <div className="version-paths">
                      {version.changed.map(path => (
                        <code key={path}>{path || '(whole value)'}</code>
                      ))}
                    </div>
                    {version.lines.length > 0 && (
                      <div className="version-lines">
                        {version.lines.map(line => (
                          <SourceLocation key={asNumber(line)} file={span?.file} line={line} />
                        ))}
                      </div>
                    )}
                  </div>
                </li>
              );
            })}
          </ol>
        )}
      </Panel>
    </div>
  );
}

function CallRow({ row }: { row: ExactInstance['calls'][number] }) {
  const calls = asBigInt(row.calls);
  const changed = asBigInt(row.changed_calls);
  return (
    <>
      <EntityLink entity={{ kind: 'span', id: asNumber(row.span) }} />
      <span>{formatInteger(calls)}</span>
      <strong className={changed > 0n ? 'warn' : ''}>{formatInteger(changed)}</strong>
      <span>{calls > 0n ? formatPercent(changed, calls) : 'nested'}</span>
      <span>{formatInteger(row.touches)}</span>
    </>
  );
}

function TypeDetail({ index, objects, typeId }: { index: ProfileIndex; objects: ObjectsIndex; typeId: number }) {
  const summary = objects.typeSummaries.find(row => row.typeId === typeId);
  const rows = useMemo(() => fieldRows(objects.fieldsByType.get(typeId) ?? []), [objects, typeId]);
  const sites = useMemo(
    () => [...(objects.sitesByType.get(typeId) ?? [])].sort((a, b) => byBigIntDesc(asBigInt(a.count), asBigInt(b.count))),
    [objects, typeId],
  );
  const top = (objects.instancesByType.get(typeId) ?? []).slice(0, 12);
  return (
    <div className="object-detail">
      <section className="panel detail-hero">
        <div className="eyebrow">TYPE</div>
        <h2>{typeName(index, typeId)}</h2>
        <div className="detail-stats">
          <div>
            <small>Instances</small>
            <strong>{summary?.instances ?? 0}</strong>
          </div>
          <div>
            <small>Changed instances</small>
            <strong>{summary?.changedInstances ?? 0}</strong>
          </div>
          <div>
            <small>Versions</small>
            <strong>{formatInteger(summary?.versions ?? 0n)}</strong>
          </div>
          <div>
            <small>Tracked paths</small>
            <strong>{rows.length}</strong>
          </div>
        </div>
        <ViewLinks entity={{ kind: 'type', id: typeId }} views={[{ view: 'raw', label: 'Raw' }]} />
      </section>
      <Panel eyebrow="FIELDS" title="Reads, writes and real changes" aside="Writes that never changed the value are highlighted">
        {rows.length === 0 ? (
          <Empty>No field counters were recorded for this type.</Empty>
        ) : (
          <div className="object-table fields-table">
            <span>Path</span>
            <span>Reads</span>
            <span>Writes</span>
            <span>Calls</span>
            <span>Changes</span>
            <span>Functions</span>
            {rows.map(row => {
              const wasted = row.writes > 0n && row.changes === 0n;
              return (
                <FieldRowCells key={row.path} depth={pathDepth(row.path)} wasted={wasted} row={row} />
              );
            })}
          </div>
        )}
        <Note>
          Reads, writes and calls count executed source sites in tracked functions. Changes count calls after which the value at that path differed,
          including changes made through aliases, callees, or interior mutability.
        </Note>
      </Panel>
      <Panel eyebrow="SOURCE" title="Access sites" aside={`${sites.length} sites`}>
        {sites.length === 0 ? (
          <Empty>No access sites were recorded; this type may only appear nested inside others.</Empty>
        ) : (
          <div className="object-table sites-table">
            <span>Function</span>
            <span>Path</span>
            <span>Access</span>
            <span>Location</span>
            <span>Count</span>
            {sites.map(site => {
              const span = index.spans.get(asNumber(site.span));
              return (
                <SiteCells key={`${site.span}:${site.path}:${site.access}:${site.line}`} span={asNumber(site.span)} path={site.path} access={site.access} file={span?.file} line={site.line} count={site.count} />
              );
            })}
          </div>
        )}
      </Panel>
      <Panel eyebrow="INSTANCES" title="Most changed" aside={`${top.length} of ${summary?.instances ?? 0}`}>
        <div className="instance-chips">
          {top.map(id => (
            <EntityLink key={id} entity={{ kind: 'instance', id }} view="stay">
              {objects.instances.get(id)!.key} · {formatInteger(objects.instances.get(id)!.versions)} v
            </EntityLink>
          ))}
        </div>
      </Panel>
    </div>
  );
}

function FieldRowCells({ row, depth, wasted }: { row: ReturnType<typeof fieldRows>[number]; depth: number; wasted: boolean }) {
  return (
    <>
      <code style={{ paddingLeft: `${depth * 14}px` }}>{row.path || '(whole value)'}</code>
      <span>{formatInteger(row.reads)}</span>
      <span className={wasted ? 'warn' : ''} title={wasted ? 'Written but never changed' : undefined}>
        {formatInteger(row.writes)}
      </span>
      <span>{formatInteger(row.calls)}</span>
      <strong>{formatInteger(row.changes)}</strong>
      <span className="field-spans">
        {row.spans.map(field => (
          <EntityLink key={asNumber(field.span)} entity={{ kind: 'span', id: asNumber(field.span) }} />
        ))}
      </span>
    </>
  );
}

function SiteCells(props: { span: number; path: string; access: string; file?: string; line: number | bigint; count: number | bigint }) {
  return (
    <>
      <EntityLink entity={{ kind: 'span', id: props.span }} />
      <code>{props.path || '(whole value)'}</code>
      <span className={`access access-${props.access}`}>{props.access}</span>
      <SourceLocation file={props.file} line={props.line} />
      <strong>{formatInteger(props.count)}</strong>
    </>
  );
}

function Summary({ index, objects }: { index: ProfileIndex; objects: ObjectsIndex }) {
  const instances = [...objects.instances.entries()]
    .sort((a, b) => byBigIntDesc(asBigInt(a[1].versions), asBigInt(b[1].versions)) || a[0] - b[0])
    .slice(0, 10);
  const wasted = objects.objects.fields
    .filter(field => asBigInt(field.writes) > 0n && asBigInt(field.changes) === 0n)
    .sort((a, b) => byBigIntDesc(asBigInt(a.writes), asBigInt(b.writes)))
    .slice(0, 12);
  return (
    <div className="object-detail">
      <Panel eyebrow="MOST CHANGED" title="Instances with the most versions">
        <div className="instance-chips">
          {instances.map(([id, instance]) => (
            <EntityLink key={id} entity={{ kind: 'instance', id }} view="stay">
              {instanceName(index, instance)} · {formatInteger(instance.versions)} v
            </EntityLink>
          ))}
        </div>
      </Panel>
      <Panel eyebrow="WASTED WORK" title="Writes that never changed a value" aside={`${wasted.length} paths`}>
        {wasted.length === 0 ? (
          <Empty>Every written path changed at least once.</Empty>
        ) : (
          <div className="object-table wasted-table">
            <span>Function</span>
            <span>Type · path</span>
            <span>Writes</span>
            {wasted.map(field => (
              <WastedCells key={`${field.span}:${field.type_id}:${field.path}`} index={index} field={field} />
            ))}
          </div>
        )}
        <Note>A write site that executes without changing the value is often an avoidable assignment or an over-eager invalidation.</Note>
      </Panel>
    </div>
  );
}

function WastedCells({ index, field }: { index: ProfileIndex; field: ObjectsIndex['objects']['fields'][number] }) {
  return (
    <>
      <EntityLink entity={{ kind: 'span', id: asNumber(field.span) }}>{spanShortName(index, asNumber(field.span))}</EntityLink>
      <span>
        <EntityLink entity={{ kind: 'type', id: asNumber(field.type_id) }} view="stay" /> <code>{field.path || '(whole value)'}</code>
      </span>
      <strong className="warn">{formatInteger(field.writes)}</strong>
    </>
  );
}

export function Objects({ index }: { index: ProfileIndex }) {
  const objects = index.objects;
  const typeId = useProfile(state => state.typeId);
  const instanceId = useProfile(state => state.instanceId);
  const navigate = useProfile(state => state.navigate);
  const colors = useThemeColors();
  const model = useMemo(() => objectModel(index), [index]);
  const selected = useMemo(() => {
    const nodes = new Set<number>();
    if (!model || !objects) return nodes;
    if (instanceId !== null) {
      const node = model.lookup.get(entityKey({ kind: 'instance', id: instanceId }));
      if (node !== undefined) nodes.add(node);
    } else if (typeId !== null) {
      for (const id of objects.instancesByType.get(typeId) ?? []) {
        const node = model.lookup.get(entityKey({ kind: 'instance', id }));
        if (node !== undefined) nodes.add(node);
      }
    }
    return nodes;
  }, [model, objects, typeId, instanceId]);
  if (!objects) return <NoObjects />;
  const capture = objects.objects.capture;
  const approximate = objects.typeSummaries.reduce((sum, row) => sum + row.approximate, 0);
  const totalVersions = objects.typeSummaries.reduce((sum, row) => sum + row.versions, 0n);
  return (
    <div className="panel-stack">
      <div className="stats-grid">
        <StatCard label="Tracked instances" value={formatInteger(objects.instances.size)} detail={`${objects.types.size} types`} accent />
        <StatCard label="Versions" value={formatInteger(totalVersions)} detail="Observed value changes" />
        <StatCard
          label="Dropped instances"
          value={formatInteger(capture.dropped_instances)}
          detail={`Limit ${formatInteger(capture.max_instances)} · history ${formatInteger(capture.history_limit)}`}
          tone={asNumber(capture.dropped_instances) > 0 ? 'warn' : ''}
        />
        <StatCard label="Approximate keys" value={approximate} detail="Keyed by memory address" tone={approximate > 0 ? 'warn' : ''} />
      </div>
      <div className="objects-layout">
        <div className="objects-side">
          <TypeList index={index} objects={objects} />
          <InstanceList index={index} objects={objects} />
        </div>
        <div className="objects-main">
          {model && (
            <Panel eyebrow="OBJECT GRAPH" title="Instances, containment and the functions that touched them" aside="Click an instance or a function" className="graph-panel">
              <GraphView
                model={model}
                selected={selected}
                onSelect={entity => navigate(entity, entity.kind === 'span' ? 'routines' : 'stay')}
                ariaLabel="Interactive object graph"
                className="object-graph"
                legend={[
                  { label: 'Instance (color: type, size: versions)', swatch: colors.palette[0] },
                  { label: 'Function', swatch: colors.accent },
                  { label: 'Contains', swatch: colors.muted, shape: 'line' },
                  { label: 'Changed by', swatch: colors.warning, shape: 'line' },
                  { label: 'Cloned into', swatch: colors.info, shape: 'dash' },
                ]}
              />
            </Panel>
          )}
          {instanceId !== null ? (
            <InstanceDetail index={index} objects={objects} instanceId={instanceId} />
          ) : typeId !== null ? (
            <TypeDetail index={index} objects={objects} typeId={typeId} />
          ) : (
            <Summary index={index} objects={objects} />
          )}
        </div>
      </div>
    </div>
  );
}
