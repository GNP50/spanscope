import { useEffect, useMemo, useRef } from 'react';
import {
  asNumber,
  formatBytes,
  formatInteger,
  formatNs,
  formatPercent,
  hasHeapData,
  label,
  memoryRoutines,
  memoryTotals,
  memoryTree,
  peakHistogram,
  rootMemorySeries,
  spanName,
  spanShortName,
  type ExactChain,
  type MemoryRoutine,
  type ProfileIndex,
} from '../model';
import { useProfile } from '../store';
import { Empty, EntityLink, Note, Panel, SourceLocation, StatCard, ViewLinks } from '../ui/components';
import { echarts, useThemeColors, type ThemeColors } from '../ui/theme';

type Option = Parameters<ReturnType<typeof echarts.init>['setOption']>[0];

/** Mounts one ECharts instance, resizes it with its box, and disposes it. */
function Chart({
  build,
  label: ariaLabel,
  height = 300,
  onPick,
}: {
  build: (narrow: boolean) => Option;
  label: string;
  height?: number;
  onPick?: (dataIndex: number, params: { data?: unknown; seriesIndex?: number }) => void;
}) {
  const container = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!container.current) return;
    const chart = echarts.init(container.current, undefined, { renderer: 'canvas' });
    chart.setOption(build(container.current.clientWidth < 520));
    if (onPick) chart.on('click', (event: { dataIndex: number; data?: unknown; seriesIndex?: number }) => onPick(event.dataIndex, event));
    const observer = new ResizeObserver(() => chart.resize());
    observer.observe(container.current);
    return () => {
      observer.disconnect();
      chart.dispose();
    };
  }, [build, onPick]);
  return <div ref={container} className="memory-chart" style={{ height }} role="img" aria-label={ariaLabel} />;
}

const axisBytes = (colors: ThemeColors) => ({
  type: 'value' as const,
  axisLabel: { hideOverlap: true, color: colors.muted, formatter: (value: number) => formatBytes(value) },
  splitLine: { lineStyle: { color: colors.line } },
});

const signedBytes = (value: bigint | number): string => {
  const number = asNumber(value);
  return number < 0 ? `−${formatBytes(-number)}` : formatBytes(number);
};

/** Peak and retained bytes of the routines with the largest peaks. */
function RoutineBars({ index, rows }: { index: ProfileIndex; rows: MemoryRoutine[] }) {
  const colors = useThemeColors();
  const navigate = useProfile(state => state.navigate);
  const top = useMemo(() => rows.slice(0, 10).reverse(), [rows]);
  const build = useMemo(
    () => (narrow: boolean): Option => {
      const labels = top.map(row => spanShortName(index, row.spanId));
      return {
        animation: false,
        backgroundColor: 'transparent',
        legend: { top: 0, right: 8, textStyle: { color: colors.muted }, itemWidth: 12, itemHeight: 8 },
        grid: { left: narrow ? 96 : 140, right: 24, top: 30, bottom: 28 },
        xAxis: { ...axisBytes(colors), splitNumber: narrow ? 2 : 4 },
        yAxis: { type: 'category', data: labels, axisLabel: { color: colors.text, width: narrow ? 86 : 128, overflow: 'truncate' }, axisTick: { show: false }, axisLine: { show: false } },
        tooltip: {
          trigger: 'axis',
          axisPointer: { type: 'shadow' },
          formatter: (params: { dataIndex: number }[]) => {
            const row = top[params[0]?.dataIndex ?? 0];
            if (!row) return '';
            return [
              `<b>${spanName(index, row.spanId)}</b>`,
              `Peak (max): ${formatBytes(row.peakMax)}`,
              `Peak (mean): ${formatBytes(row.calls ? Number(row.peakSum / row.calls) : 0)}`,
              `Net retained: ${signedBytes(row.netBytes)}`,
              `Allocated incl.: ${formatBytes(row.inclAllocBytes)}`,
            ].join('<br/>');
          },
        },
        series: [
          { name: 'Peak per call (max)', type: 'bar', data: top.map(row => asNumber(row.peakMax)), barMaxWidth: 12, itemStyle: { color: colors.info, borderRadius: [0, 3, 3, 0] } },
          {
            name: 'Net retained (total)',
            type: 'bar',
            data: top.map(row => Math.max(0, asNumber(row.netBytes))),
            barMaxWidth: 12,
            itemStyle: { color: colors.warning, borderRadius: [0, 3, 3, 0] },
          },
        ],
      };
    },
    [top, index, colors],
  );
  const pick = useMemo(() => (dataIndex: number) => {
    const row = top[dataIndex];
    if (row) navigate({ kind: 'span', id: row.spanId }, 'stay');
  }, [top, navigate]);
  return <Chart build={build} onPick={pick} label="Peak and retained bytes by routine" height={Math.max(220, top.length * 34 + 60)} />;
}

/** Per-run net change, peak, and the running total of retained bytes. */
function RunSeries({ index }: { index: ProfileIndex }) {
  const colors = useThemeColors();
  const navigate = useProfile(state => state.navigate);
  const series = useMemo(() => rootMemorySeries(index), [index]);
  const build = useMemo(
    () => (narrow: boolean): Option => ({
      animation: false,
      backgroundColor: 'transparent',
      legend: { top: 0, right: 8, textStyle: { color: colors.muted }, itemWidth: 12, itemHeight: 8 },
      grid: { left: narrow ? 60 : 78, right: 20, top: 34, bottom: 34 },
      xAxis: {
        type: 'category',
        data: series.map((_, position) => `#${position + 1}`),
        axisLabel: { color: colors.muted, hideOverlap: true },
        axisLine: { lineStyle: { color: colors.line } },
      },
      yAxis: { ...axisBytes(colors), splitNumber: 4 },
      tooltip: {
        trigger: 'axis',
        formatter: (params: { dataIndex: number }[]) => {
          const point = series[params[0]?.dataIndex ?? 0];
          if (!point) return '';
          return [
            `<b>Run #${(params[0]?.dataIndex ?? 0) + 1} · ${spanName(index, point.spanId)}</b>`,
            `Started at ${formatNs(point.startNs)}`,
            `Peak: ${formatBytes(point.peakBytes)}`,
            `Net: ${signedBytes(point.netBytes)}`,
            `Cumulative net: ${signedBytes(point.cumulativeNetBytes)}`,
          ].join('<br/>');
        },
      },
      series: [
        { name: 'Peak', type: 'bar', data: series.map(point => asNumber(point.peakBytes)), barMaxWidth: 16, itemStyle: { color: colors.info, opacity: 0.75 } },
        { name: 'Net', type: 'bar', data: series.map(point => asNumber(point.netBytes)), barMaxWidth: 16, itemStyle: { color: colors.warning } },
        {
          name: 'Cumulative net',
          type: 'line',
          data: series.map(point => asNumber(point.cumulativeNetBytes)),
          symbolSize: 5,
          lineStyle: { color: colors.danger, width: 2 },
          itemStyle: { color: colors.danger },
          areaStyle: { color: colors.danger, opacity: 0.08 },
        },
      ],
    }),
    [series, index, colors],
  );
  const pick = useMemo(() => (dataIndex: number) => {
    const point = series[dataIndex];
    if (point) navigate({ kind: 'root', uid: point.uid }, 'runs');
  }, [series, navigate]);
  if (series.length === 0) return <Empty>No retained runs carry heap data.</Empty>;
  return <Chart build={build} onPick={pick} label="Heap balance across retained runs" />;
}

/** Allocation count per call against average size; bubble area is total bytes. */
function ChurnScatter({ index, rows }: { index: ProfileIndex; rows: MemoryRoutine[] }) {
  const colors = useThemeColors();
  const navigate = useProfile(state => state.navigate);
  const points = useMemo(() => rows.filter(row => row.allocs > 0n && row.calls > 0n), [rows]);
  const build = useMemo(
    () => (narrow: boolean): Option => {
      const largest = Math.max(1, ...points.map(row => asNumber(row.allocBytes)));
      return {
        animation: false,
        backgroundColor: 'transparent',
        grid: { left: narrow ? 56 : 70, right: 24, top: 16, bottom: 44 },
        xAxis: {
          type: 'log',
          name: 'allocations per call',
          nameLocation: 'middle',
          nameGap: 28,
          nameTextStyle: { color: colors.muted },
          axisLabel: { color: colors.muted },
          splitLine: { lineStyle: { color: colors.line } },
        },
        yAxis: {
          type: 'log',
          name: 'average size',
          nameTextStyle: { color: colors.muted },
          axisLabel: { color: colors.muted, formatter: (value: number) => formatBytes(value) },
          splitLine: { lineStyle: { color: colors.line } },
        },
        tooltip: {
          trigger: 'item',
          formatter: (params: { dataIndex: number }) => {
            const row = points[params.dataIndex];
            if (!row) return '';
            return [
              `<b>${spanName(index, row.spanId)}</b>`,
              `${formatInteger(row.allocs)} allocations, ${formatInteger(row.frees)} frees`,
              `${(asNumber(row.allocs) / asNumber(row.calls)).toFixed(1)} per call`,
              `Average size: ${formatBytes(Math.round(asNumber(row.allocBytes) / asNumber(row.allocs)))}`,
              `Freed: ${formatPercent(row.freedBytes, row.allocBytes)} of direct bytes`,
            ].join('<br/>');
          },
        },
        series: [
          {
            type: 'scatter',
            data: points.map(row => [
              Math.max(0.1, asNumber(row.allocs) / asNumber(row.calls)),
              Math.max(1, asNumber(row.allocBytes) / asNumber(row.allocs)),
            ]),
            symbolSize: (_: unknown, params: { dataIndex: number }) =>
              8 + 34 * Math.sqrt(asNumber(points[params.dataIndex]?.allocBytes ?? 0) / largest),
            itemStyle: { color: colors.accent, opacity: 0.7, borderColor: colors.surface },
            label: {
              show: !narrow,
              position: 'right',
              color: colors.muted,
              fontSize: 10,
              formatter: (params: { dataIndex: number }) => spanShortName(index, points[params.dataIndex]?.spanId ?? 0),
            },
          },
        ],
      };
    },
    [points, index, colors],
  );
  const pick = useMemo(() => (dataIndex: number) => {
    const row = points[dataIndex];
    if (row) navigate({ kind: 'span', id: row.spanId }, 'stay');
  }, [points, navigate]);
  if (points.length === 0) return <Empty>No direct allocations were attributed.</Empty>;
  return <Chart build={build} onPick={pick} label="Allocation count and size by routine" />;
}

/** Inclusive allocated bytes along the call hierarchy. */
function AllocationSunburst({ index, chains }: { index: ProfileIndex; chains: ExactChain[] }) {
  const colors = useThemeColors();
  const navigate = useProfile(state => state.navigate);
  const tree = useMemo(() => memoryTree(chains, id => spanShortName(index, id)), [index, chains]);
  const build = useMemo(
    () => (): Option => ({
      animation: false,
      backgroundColor: 'transparent',
      color: colors.palette,
      tooltip: {
        trigger: 'item',
        formatter: (params: { data?: { spanId?: number; value?: number } }) =>
          params.data?.spanId === undefined ? '' : `<b>${spanName(index, params.data.spanId)}</b><br/>Allocated incl.: ${formatBytes(params.data.value ?? 0)}`,
      },
      series: [
        {
          type: 'sunburst',
          data: tree,
          radius: ['12%', '92%'],
          sort: undefined,
          nodeClick: false,
          itemStyle: { borderColor: colors.surface, borderWidth: 2 },
          label: { color: colors.ink, fontSize: 10, minAngle: 12, rotate: 'radial' },
          emphasis: { focus: 'ancestor' },
        },
      ],
    }),
    [tree, index, colors],
  );
  const pick = useMemo(() => (_: number, params: { data?: unknown }) => {
    const spanId = (params.data as { spanId?: number } | undefined)?.spanId;
    if (spanId !== undefined) navigate({ kind: 'span', id: spanId }, 'stay');
  }, [navigate]);
  if (tree.length === 0) return <Empty>No allocations match the current filters.</Empty>;
  return <Chart build={build} onPick={pick} label="Allocated bytes along the call hierarchy" height={340} />;
}

/** Distribution of per-call peaks for the selected routine. */
function PeakDistribution({ index, spanId }: { index: ProfileIndex; spanId: number }) {
  const colors = useThemeColors();
  const buckets = useMemo(() => peakHistogram(index, spanId), [index, spanId]);
  const build = useMemo(
    () => (): Option => ({
      animation: false,
      backgroundColor: 'transparent',
      grid: { left: 48, right: 16, top: 14, bottom: 46 },
      xAxis: {
        type: 'category',
        data: buckets.map(bucket => `≤ ${formatBytes(bucket.upperBytes)}`),
        axisLabel: { color: colors.muted, rotate: 30, hideOverlap: true },
        axisLine: { lineStyle: { color: colors.line } },
      },
      yAxis: { type: 'value', minInterval: 1, axisLabel: { color: colors.muted }, splitLine: { lineStyle: { color: colors.line } } },
      tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' } },
      series: [{ name: 'Calls', type: 'bar', data: buckets.map(bucket => asNumber(bucket.count)), barMaxWidth: 22, itemStyle: { color: colors.accent, borderRadius: [3, 3, 0, 0] } }],
    }),
    [buckets, colors],
  );
  if (buckets.length === 0) return <Empty>No per-call peaks recorded for this routine.</Empty>;
  return <Chart build={build} label="Per-call heap peak distribution" height={240} />;
}

function RoutineTable({ index, rows, selected }: { index: ProfileIndex; rows: MemoryRoutine[]; selected: number | null }) {
  const navigate = useProfile(state => state.navigate);
  return (
    <div className="memory-table-wrap">
      <table className="memory-table">
        <thead>
          <tr>
            <th>Routine</th>
            <th>Calls</th>
            <th title="Allocated while the routine itself was running">Allocated</th>
            <th title="Freed while the routine itself was running">Freed</th>
            <th title="Allocated including children">Allocated incl.</th>
            <th title="Inclusive allocated minus freed, all calls">Net</th>
            <th title="Largest per-call heap high-water mark">Peak max</th>
            <th title="Mean per-call high-water mark">Peak mean</th>
            <th title="Calls that returned with more heap than at entry">Retaining</th>
            <th>Allocs / call</th>
          </tr>
        </thead>
        <tbody>
          {rows.map(row => (
            <tr key={row.spanId} className={selected === row.spanId ? 'selected' : ''} onClick={() => navigate({ kind: 'span', id: row.spanId }, 'stay')}>
              <td title={spanName(index, row.spanId)}>{spanShortName(index, row.spanId)}</td>
              <td>{formatInteger(row.calls)}</td>
              <td>{formatBytes(row.allocBytes)}</td>
              <td>{formatBytes(row.freedBytes)}</td>
              <td>{formatBytes(row.inclAllocBytes)}</td>
              <td className={row.netBytes > 0n ? 'warn' : ''}>{signedBytes(row.netBytes)}</td>
              <td>{formatBytes(row.peakMax)}</td>
              <td>{formatBytes(row.calls ? Number(row.peakSum / row.calls) : 0)}</td>
              <td>{formatPercent(row.retainingCalls, row.calls)}</td>
              <td>{row.calls ? (asNumber(row.allocs) / asNumber(row.calls)).toFixed(1) : '—'}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function MemoryInsights({ index }: { index: ProfileIndex }) {
  const navigate = useProfile(state => state.navigate);
  const insights = index.profile.analysis.insights.filter(item => item.kind.startsWith('memory.') || item.kind.startsWith('allocation.'));
  if (insights.length === 0) return <Empty>No memory diagnostics fired for this capture.</Empty>;
  return (
    <div className="memory-insights">
      {insights.map((item, position) => {
        const spanId = item.evidence.spans.length ? asNumber(item.evidence.spans[0]) : null;
        return (
          <button
            key={`${item.kind}:${position}`}
            className={`memory-insight severity-${item.severity}`}
            onClick={() => spanId !== null && navigate({ kind: 'span', id: spanId }, 'stay')}
          >
            <span className="insight-severity">{item.severity}</span>
            <span>
              <small>{label(item.kind.split('.').pop() ?? item.kind)}</small>
              <strong>{item.title}</strong>
              <p>{item.explanation}</p>
            </span>
          </button>
        );
      })}
    </div>
  );
}

export function Memory({ index }: { index: ProfileIndex }) {
  // Whole-capture view: selecting a routine highlights it instead of filtering totals away.
  const chains = index.profile.chains;
  const spanId = useProfile(state => state.spanId);
  const rows = useMemo(() => memoryRoutines(index, chains), [index, chains]);
  const totals = useMemo(() => memoryTotals(index, chains), [index, chains]);
  if (!hasHeapData(index.profile)) {
    return (
      <div className="panel-stack">
        <Panel eyebrow="HEAP BALANCE" title="No heap data in this profile">
          <Empty>
            Build with the <code>alloc-tracker</code> feature and install <code>spanscope::TrackingAllocator</code> as the
            <code> #[global_allocator]</code> to record allocated, freed, net and peak bytes for every span.
          </Empty>
        </Panel>
      </div>
    );
  }
  const selected = spanId !== null ? rows.find(row => row.spanId === spanId) ?? null : null;
  const span = selected ? index.spans.get(selected.spanId) : null;
  return (
    <div className="panel-stack">
      <div className="stats-grid">
        <StatCard label="Allocated" value={formatBytes(totals.allocBytes)} detail={`${formatInteger(totals.allocs)} allocations`} accent />
        <StatCard label="Freed" value={formatBytes(totals.freedBytes)} detail={`${formatInteger(totals.frees)} deallocations`} />
        <StatCard
          label="Net retained"
          value={signedBytes(totals.netBytes)}
          detail={
            totals.retainingRoutines === 0
              ? 'No routine mostly retains memory'
              : `${totals.retainingRoutines} ${totals.retainingRoutines === 1 ? 'routine mostly retains' : 'routines mostly retain'} memory`
          }
          tone={totals.netBytes > 0n ? 'warn' : 'good'}
        />
        <StatCard label="Largest peak" value={formatBytes(totals.peakMax)} detail="Highest single-call high-water mark" />
      </div>
      <div className="memory-grid">
        <Panel eyebrow="BY ROUTINE" title="Who needs the most memory" aside="Click a bar to select">
          {rows.length ? <RoutineBars index={index} rows={rows} /> : <Empty>No chains match the current filters.</Empty>}
        </Panel>
        <Panel eyebrow="ACROSS RUNS" title="Does memory grow over time?" aside="A rising red line means runs keep memory">
          <RunSeries index={index} />
        </Panel>
        <Panel eyebrow="CALL HIERARCHY" title="Where allocations come from" aside="Inclusive allocated bytes">
          <AllocationSunburst index={index} chains={chains} />
        </Panel>
        <Panel eyebrow="CHURN" title="Many small allocations or few large ones?" aside="Bubble area: direct bytes">
          <ChurnScatter index={index} rows={rows} />
        </Panel>
      </div>
      {selected && span && (
        <section className="panel selection-strip">
          <div>
            <div className="eyebrow">SELECTED ROUTINE</div>
            <h3>{span.name}</h3>
            <SourceLocation file={span.file} line={span.line} />
          </div>
          <div className="strip-stats">
            <span>
              <small>Peak p99</small>
              <strong>{formatBytes(selected.peakP99)}</strong>
            </span>
            <span>
              <small>Net per call</small>
              <strong>
                {signedBytes(selected.netMin)} … {signedBytes(selected.netMax)}
              </strong>
            </span>
            <span>
              <small>Retaining calls</small>
              <strong>{formatPercent(selected.retainingCalls, selected.calls)}</strong>
            </span>
          </div>
          <ViewLinks
            entity={{ kind: 'span', id: selected.spanId }}
            views={[
              { view: 'routines', label: 'Routine' },
              { view: 'flame', label: 'Flame' },
              { view: 'chains', label: 'Chains' },
            ]}
          />
        </section>
      )}
      <div className="memory-grid">
        <Panel
          eyebrow="DISTRIBUTION"
          title={selected ? `Per-call peak of ${spanShortName(index, selected.spanId)}` : 'Per-call peak'}
          aside={selected ? `${formatInteger(selected.calls)} calls` : 'Select a routine'}
        >
          {selected ? <PeakDistribution index={index} spanId={selected.spanId} /> : <Empty>Select a routine in a chart or the table below.</Empty>}
        </Panel>
        <Panel eyebrow="DIAGNOSTICS" title="Memory insights">
          <MemoryInsights index={index} />
        </Panel>
      </div>
      <Panel eyebrow="TABLE" title="Heap balance by routine" aside={`${rows.length} routines`}>
        <RoutineTable index={index} rows={rows} selected={spanId} />
        <Note>
          Sizes are requested bytes. A free is charged to the routine that frees the block, so a routine that returns data shows positive net and
          its caller the matching release. Peak is the high-water mark above the heap level at entry, including children.{' '}
          {selected && <EntityLink entity={{ kind: 'span', id: selected.spanId }} view="routines" />}
        </Note>
      </Panel>
    </div>
  );
}
