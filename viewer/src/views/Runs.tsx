import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { asNumber, chainName, formatInteger, formatNs, rootName, spanName, type ExactRoot, type ProfileIndex } from '../model';
import { metricNames, summarizeRoutines } from '../model/routines';
import { useProfile } from '../store';
import { Empty, EntityLink, Note, Panel, RunPicker, WeightBar } from '../ui/components';
import { echarts, useThemeColors } from '../ui/theme';

function MetricsChart({ index, metric, selectedRoot }: { index: ProfileIndex; metric: string; selectedRoot: string | null }) {
  const element = useRef<HTMLDivElement>(null);
  const navigate = useProfile(state => state.navigate);
  const colors = useThemeColors();
  useEffect(() => {
    if (!element.current || !metric) return;
    const chart = echarts.init(element.current);
    const roots = index.profile.roots.filter(root => Object.hasOwn(root.metrics, metric));
    chart.setOption({
      animation: false,
      grid: { left: 55, right: 15, top: 20, bottom: 60 },
      xAxis: {
        type: 'category',
        data: roots.map((root, i) => `${i + 1} · ${root.uid.slice(0, 6)}`),
        axisLabel: { color: colors.muted, rotate: roots.length > 8 ? 45 : 0 },
      },
      yAxis: { type: 'value', axisLabel: { color: colors.muted }, splitLine: { lineStyle: { color: colors.line } } },
      tooltip: {
        trigger: 'item',
        formatter: (params: { dataIndex: number }) => `${roots[params.dataIndex]?.uid}<br/>${metric}: ${roots[params.dataIndex]?.metrics[metric]}`,
      },
      series: [
        {
          type: 'bar',
          data: roots.map(root => ({ value: root.metrics[metric], itemStyle: { color: root.uid === selectedRoot ? colors.highlight : colors.accent } })),
          barMaxWidth: 42,
        },
      ],
    });
    chart.on('click', (event: { dataIndex: number }) => {
      const root = roots[event.dataIndex];
      if (root) navigate({ kind: 'root', uid: root.uid }, 'stay');
    });
    const observer = new ResizeObserver(() => chart.resize());
    observer.observe(element.current);
    return () => {
      observer.disconnect();
      chart.dispose();
    };
  }, [index, metric, selectedRoot, navigate, colors]);
  return <div className="metric-chart" ref={element} role="img" aria-label={`${metric} across retained runs`} />;
}

function InvocationExplorer({ index, root }: { index: ProfileIndex; root: ExactRoot }) {
  const [query, setQuery] = useState('');
  const [order, setOrder] = useState<'start' | 'duration'>('start');
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const navigate = useProfile(state => state.navigate);
  const scroll = useRef<HTMLDivElement>(null);
  const needle = query.toLowerCase();
  const span = (item: { t_start_ns: number | bigint; t_end_ns: number | bigint }) => asNumber(item.t_end_ns) - asNumber(item.t_start_ns);
  const invocations = useMemo(
    () =>
      root.execution.invocations
        .filter(invocation => index.chains.get(asNumber(invocation.chain))?.path.some(id => spanName(index, asNumber(id)).toLowerCase().includes(needle)) ?? false)
        .sort((a, b) => (order === 'duration' ? span(b) - span(a) : asNumber(a.t_start_ns) - asNumber(b.t_start_ns))),
    [index, root, needle, order],
  );
  const virtualizer = useVirtualizer({ count: invocations.length, getScrollElement: () => scroll.current, estimateSize: () => 54, overscan: 12 });
  const selected = invocations.find(invocation => asNumber(invocation.id) === selectedId) ?? invocations[0];
  const selectedChain = selected ? index.chains.get(asNumber(selected.chain)) : null;
  const rootStart = asNumber(root.t_start_ns);
  const rootDuration = Math.max(1, asNumber(root.duration_ns));
  return (
    <Panel eyebrow="BOUNDED INVOCATION EVIDENCE" title="Individual calls" aside={`${formatInteger(root.execution.invocations.length)} retained · ${root.execution.status}`}>
      <div className="invocation-controls">
        <input aria-label="Search individual calls" placeholder="Search a routine or call path…" value={query} onChange={event => setQuery(event.target.value)} />
        <select aria-label="Order individual calls" value={order} onChange={event => setOrder(event.target.value as 'start' | 'duration')}>
          <option value="start">Start time</option>
          <option value="duration">Longest first</option>
        </select>
      </div>
      <div className="invocation-layout">
        <div ref={scroll} className="invocation-scroll">
          <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
            {virtualizer.getVirtualItems().map(item => {
              const call = invocations[item.index];
              const duration = Math.max(0, span(call));
              const start = Math.max(0, asNumber(call.t_start_ns) - rootStart);
              return (
                <button
                  key={asNumber(call.id)}
                  className={`invocation-row ${selected?.id === call.id ? 'selected' : ''}`}
                  style={{ transform: `translateY(${item.start}px)` }}
                  onClick={() => setSelectedId(asNumber(call.id))}
                >
                  <span className="invocation-name">
                    <strong>
                      #{formatInteger(call.id)} {chainName(index, asNumber(call.chain))}
                    </strong>
                    <small>
                      Thread #{formatInteger(call.thread)} · +{formatNs(start)}
                    </small>
                  </span>
                  <span className="invocation-duration">{formatNs(duration)}</span>
                  <span className="invocation-track">
                    <span
                      style={{
                        left: `${Math.min(100, (start / rootDuration) * 100)}%`,
                        width: `${Math.max(1, Math.min(100, (duration / rootDuration) * 100))}%`,
                      }}
                    />
                  </span>
                </button>
              );
            })}
          </div>
        </div>
        <div className="invocation-detail">
          {selected && (
            <>
              <div className="eyebrow">CALL #{formatInteger(selected.id)}</div>
              <h3>{chainName(index, asNumber(selected.chain))}</h3>
              <div>
                <span>Inclusive wall</span>
                <strong>{formatNs(span(selected))}</strong>
              </div>
              <div>
                <span>Started in run</span>
                <strong>+{formatNs(Math.max(0, asNumber(selected.t_start_ns) - rootStart))}</strong>
              </div>
              <div>
                <span>Thread</span>
                <strong>#{formatInteger(selected.thread)}</strong>
              </div>
              <div>
                <span>Captured parent</span>
                <strong>{selected.parent === null || selected.parent === undefined ? 'Unavailable' : `#${formatInteger(selected.parent)}`}</strong>
              </div>
              <div className="view-links">
                <EntityLink entity={{ kind: 'chain', id: asNumber(selected.chain) }} view="routines">
                  Routine →
                </EntityLink>
                <EntityLink entity={{ kind: 'chain', id: asNumber(selected.chain) }} view="chains">
                  Chain →
                </EntityLink>
                <button className="entity-link" onClick={() => navigate({ kind: 'root', uid: root.uid }, 'timeline')}>
                  Timeline →
                </button>
              </div>
              <p>Call path</p>
              <ol>{selectedChain?.path.map((id, i) => <li key={i}>{spanName(index, asNumber(id))}</li>)}</ol>
            </>
          )}
        </div>
      </div>
      <Note>
        Rows show recorded invocation intervals, not necessarily every call made by the application. A partial or truncated evidence status means some
        intervals or causal links are missing. Per-invocation exclusive time is not available in this record.
      </Note>
    </Panel>
  );
}

export function Runs({ index }: { index: ProfileIndex }) {
  const rootUid = useProfile(state => state.rootUid);
  const navigate = useProfile(state => state.navigate);
  const names = useMemo(() => metricNames(index), [index]);
  const [metric, setMetric] = useState('');
  const activeMetric = names.includes(metric) ? metric : (names[0] ?? '');
  const selected = index.roots.get(rootUid ?? '') ?? index.profile.roots[0];
  const rows = useMemo(() => (selected ? summarizeRoutines(index, selected.uid) : []), [index, selected]);
  return (
    <div className="panel-stack">
      <Panel eyebrow="RETAINED ROOT INVOCATIONS" title="Choose a run" aside={`${index.profile.roots.length} runs · counts are local to each run`}>
        <RunPicker index={index} selected={selected ?? null} />
      </Panel>
      {selected && (
        <>
          <section className="panel detail-hero">
            <div className="eyebrow">
              RUN BREAKDOWN · {selected.completion} · {selected.execution.status} evidence
            </div>
            <h2>{rootName(index, selected)}</h2>
            <p>
              {selected.uid} · origin thread #{selected.thread}
            </p>
            <div className="detail-stats">
              <div>
                <small>Root wall duration</small>
                <strong>{formatNs(selected.duration_ns)}</strong>
              </div>
              <div>
                <small>Observed routine calls</small>
                <strong>{formatInteger(rows.reduce((total, row) => total + row.calls, 0n))}</strong>
              </div>
              <div>
                <small>Distinct routines</small>
                <strong>{rows.length}</strong>
              </div>
              <div>
                <small>Start offset</small>
                <strong>{formatNs(selected.t_start_ns)}</strong>
              </div>
            </div>
            <div className="view-links">
              <button className="entity-link" onClick={() => navigate({ kind: 'root', uid: selected.uid }, 'timeline')}>
                Timeline →
              </button>
              <button className="entity-link" onClick={() => navigate({ kind: 'root', uid: selected.uid }, 'dependencies')}>
                Graph →
              </button>
              <button className="entity-link" onClick={() => navigate({ kind: 'root', uid: selected.uid }, 'raw')}>
                Raw →
              </button>
            </div>
          </section>
          <div className="runs-grid">
            <Panel eyebrow="WHAT THIS RUN SPENT" title="Routine composition" aside="Select a routine for callers and paths">
              <div className="composition-list">
                {rows.map(row => (
                  <button key={row.spanId} onClick={() => navigate({ kind: 'span', id: row.spanId }, 'routines')}>
                    <span>
                      <strong>{spanName(index, row.spanId)}</strong>
                      <WeightBar value={row.selfActiveNs} max={rows[0]?.selfActiveNs ?? 0n} />
                    </span>
                    <strong>{formatInteger(row.calls)} calls</strong>
                    <span>{formatNs(row.selfActiveNs)} self</span>
                    <span>{formatNs(row.inclusiveWallNs)} wall sum</span>
                  </button>
                ))}
              </div>
            </Panel>
            <Panel eyebrow="RECORDED FEATURES" title="Metrics and context">
              {selected.outcome && (
                <div className="run-outcome">
                  <span>Outcome</span>
                  <strong>{selected.outcome}</strong>
                </div>
              )}
              {Object.keys(selected.attributes ?? {}).length > 0 && (
                <div className="attribute-list">
                  {Object.entries(selected.attributes ?? {}).map(([key, value]) => (
                    <div key={key}>
                      <span>{key}</span>
                      <strong>{value}</strong>
                    </div>
                  ))}
                </div>
              )}
              {Object.keys(selected.metrics).length ? (
                <div className="metric-values">
                  {Object.entries(selected.metrics).map(([key, value]) => (
                    <div key={key}>
                      <span>{key}</span>
                      <strong>{value.toLocaleString()}</strong>
                    </div>
                  ))}
                </div>
              ) : (
                <Empty>No numeric metrics were recorded for this run.</Empty>
              )}
              <div className="metric-values">
                <div>
                  <span>RSS at entry</span>
                  <strong>{selected.rss_entry_kb ? `${formatInteger(selected.rss_entry_kb)} KiB` : 'Unavailable'}</strong>
                </div>
                <div>
                  <span>RSS at exit</span>
                  <strong>{selected.rss_exit_kb ? `${formatInteger(selected.rss_exit_kb)} KiB` : 'Unavailable'}</strong>
                </div>
              </div>
              <Note>Attributes and outcome describe this root invocation. Repeated keys keep their last value. RSS requires the memory feature on Linux.</Note>
            </Panel>
          </div>
          <InvocationExplorer key={selected.uid} index={index} root={selected} />
          <Panel
            eyebrow="METRICS ACROSS RUNS"
            title="Compare recorded values"
            aside={
              names.length > 0 ? (
                <select aria-label="Metric to compare" value={activeMetric} onChange={event => setMetric(event.target.value)}>
                  {names.map(value => (
                    <option key={value} value={value}>
                      {value}
                    </option>
                  ))}
                </select>
              ) : undefined
            }
          >
            {names.length ? <MetricsChart index={index} metric={activeMetric} selectedRoot={selected.uid} /> : <Empty>No root metrics were recorded in this profile.</Empty>}
          </Panel>
        </>
      )}
    </div>
  );
}
