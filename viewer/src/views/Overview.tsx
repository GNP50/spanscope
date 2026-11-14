import { useEffect, useRef } from 'react';
import { asNumber, formatInteger, formatNs, leafSpan, rootName, spanShortName, type ExactChain, type ProfileIndex } from '../model';
import { useProfile } from '../store';
import { Empty, Note, Panel, StatCard } from '../ui/components';
import { echarts, useThemeColors } from '../ui/theme';

function OverviewChart({ index, chains }: { index: ProfileIndex; chains: ExactChain[] }) {
  const container = useRef<HTMLDivElement>(null);
  const navigate = useProfile(state => state.navigate);
  const colors = useThemeColors();
  useEffect(() => {
    if (!container.current) return;
    const chart = echarts.init(container.current, undefined, { renderer: 'canvas' });
    const narrow = container.current.clientWidth < 500;
    const top = [...chains].sort((a, b) => asNumber(b.self_ns) - asNumber(a.self_ns)).slice(0, 8).reverse();
    const labels = top.map(chain => spanShortName(index, leafSpan(chain)));
    chart.setOption({
      animation: false,
      backgroundColor: 'transparent',
      grid: { left: narrow ? 96 : 125, right: 24, top: 8, bottom: 30 },
      xAxis: {
        type: 'value',
        splitNumber: narrow ? 2 : 5,
        axisLabel: { hideOverlap: true, color: colors.muted, formatter: (value: number) => formatNs(value) },
        splitLine: { lineStyle: { color: colors.line } },
      },
      yAxis: {
        type: 'category',
        data: labels,
        axisLabel: { color: colors.text, width: 110, overflow: 'truncate' },
        axisLine: { show: false },
        axisTick: { show: false },
      },
      tooltip: {
        trigger: 'item',
        formatter: (params: { dataIndex: number }) => `${labels[params.dataIndex]}<br/>Self: ${formatNs(top[params.dataIndex].self_ns)}`,
      },
      series: [
        {
          type: 'bar',
          data: top.map(chain => asNumber(chain.self_ns)),
          barMaxWidth: 18,
          itemStyle: { color: colors.accent, borderRadius: [0, 4, 4, 0] },
        },
      ],
    });
    chart.on('click', (event: { dataIndex: number }) => {
      const chain = top[event.dataIndex];
      if (chain) navigate({ kind: 'chain', id: asNumber(chain.id) }, 'chains');
    });
    const observer = new ResizeObserver(() => chart.resize());
    observer.observe(container.current);
    return () => {
      observer.disconnect();
      chart.dispose();
    };
  }, [chains, index, navigate, colors]);
  return <div ref={container} className="overview-chart" role="img" aria-label="Top call chains by self time" />;
}

export function Overview({ index, chains }: { index: ProfileIndex; chains: ExactChain[] }) {
  const capture = index.profile.meta.capture;
  const navigate = useProfile(state => state.navigate);
  const setView = useProfile(state => state.setView);
  const objects = index.objects;
  const quickPaths = [
    { view: 'insights' as const, title: 'What deserves attention?', detail: `${index.profile.analysis.insights.length} deterministic insights →` },
    { view: 'routines' as const, title: 'Which routine costs most?', detail: 'Calls, self weight, callers and children →' },
    { view: 'runs' as const, title: 'What happened in one run?', detail: 'Per-run composition and metrics →' },
    ...(index.profile.analysis.sections.clusters
      ? [{ view: 'patterns' as const, title: 'Which runs behave differently?', detail: 'Shapes, outliers, shifts and growth →' }]
      : []),
    { view: 'dependencies' as const, title: 'Who calls whom?', detail: 'Interactive graph and direct edges →' },
    ...(objects
      ? [{ view: 'objects' as const, title: 'What changed which object?', detail: `${objects.instances.size} tracked instances →` }]
      : []),
  ];
  return (
    <div className="panel-stack">
      <div className="stats-grid">
        <StatCard label="Capture duration" value={formatNs(index.profile.meta.duration_ns)} detail={index.profile.meta.started_at} accent />
        <StatCard label="Observed calls" value={formatInteger(index.totalCalls)} detail={`${index.profile.chains.length} unique chains`} />
        <StatCard label="Active self time" value={formatNs(index.totalSelfNs)} detail="Across all chains" />
        <StatCard label="Root runs" value={formatInteger(index.profile.roots.length)} detail={`${index.profile.threads.length} participating threads`} />
      </div>
      <div className="quick-paths">
        {quickPaths.map(path => (
          <button key={path.view} onClick={() => setView(path.view)}>
            <strong>{path.title}</strong>
            <span>{path.detail}</span>
          </button>
        ))}
      </div>
      <div className="overview-grid">
        <Panel eyebrow="HOT PATHS" title="Where time accumulates" aside="Exclusive active time · click a bar" className="chart-panel">
          {chains.length ? <OverviewChart index={index} chains={chains} /> : <div className="no-results">No chains match the current filters.</div>}
        </Panel>
        <Panel eyebrow="CAPTURE QUALITY" title="What was observed" className="health-panel">
          <div className="health-row">
            <span>Snapshot</span>
            <strong className={capture.snapshot_complete ? 'good' : 'warn'}>{capture.snapshot_complete ? 'Complete' : 'Pending threads'}</strong>
          </div>
          <div className="health-row">
            <span>Sampling rate</span>
            <strong>{Math.round(asNumber(capture.sample_rate) * 100)}%</strong>
          </div>
          <div className="health-row">
            <span>Sampled out</span>
            <strong>{formatInteger(capture.sampled_out_roots)}</strong>
          </div>
          <div className="health-row">
            <span>Dropped records</span>
            <strong>{formatInteger(capture.dropped_records)}</strong>
          </div>
          <div className="health-row">
            <span>Evicted roots</span>
            <strong>{formatInteger(capture.evicted_roots)}</strong>
          </div>
          {objects && (
            <div className="health-row">
              <span>Dropped object instances</span>
              <strong className={asNumber(objects.objects.capture.dropped_instances) ? 'warn' : ''}>
                {formatInteger(objects.objects.capture.dropped_instances)}
              </strong>
            </div>
          )}
          <Note>
            Wall times can overlap across threads. Evidence can be partial or truncated; Timeline derives a causal path only from explicit segment
            dependencies.
          </Note>
        </Panel>
      </div>
      <Panel eyebrow="RUNS" title="Recent roots" aside={`${index.profile.roots.length} retained`} className="roots-panel">
        <div className="root-list">
          {index.profile.roots.length === 0 && <Empty>No root runs were retained.</Empty>}
          {index.profile.roots
            .slice(-8)
            .reverse()
            .map(root => (
              <button key={root.uid} className="root-row" onClick={() => navigate({ kind: 'root', uid: root.uid }, 'runs')}>
                <span className="root-icon">◇</span>
                <span className="root-name">
                  {rootName(index, root)}
                  <small>{root.uid}</small>
                </span>
                <span className={`root-status ${root.completion}`}>{root.completion}</span>
                <strong>{formatNs(root.duration_ns)}</strong>
                <span className="root-arrow">↗</span>
              </button>
            ))}
        </div>
      </Panel>
    </div>
  );
}
