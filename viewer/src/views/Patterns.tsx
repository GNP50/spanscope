import { useEffect, useMemo, useRef, useState } from 'react';
import { asNumber, formatInteger, formatNs, spanName, type ProfileIndex } from '../model';
import { fittedCurve, parsePatterns, type GrowthFit, type Patterns as PatternData, type SectionInfo } from '../model/patterns';
import { useProfile } from '../store';
import { Empty, EntityLink, Note, Panel, StatCard } from '../ui/components';
import { echarts, useThemeColors } from '../ui/theme';

const SHAPE = 'ABCDEFGH';
const shapeName = (id: number, count: number) => `Shape ${SHAPE[id] ?? id}${id === 0 && count > 1 ? ' · fastest' : id === count - 1 && count > 1 ? ' · slowest' : ''}`;

function SectionState({ section, children }: { section: SectionInfo | null; children: React.ReactNode }) {
  if (!section) return <Empty>This section was not computed.</Empty>;
  if (section.status !== 'complete') return <Empty>{section.explanation}</Empty>;
  return (
    <>
      {children}
      <Note>{section.explanation}</Note>
    </>
  );
}

/** Every run in start order: duration, cluster color, outliers, and level shifts. */
function RunMap({ index, patterns }: { index: ProfileIndex; patterns: PatternData }) {
  const element = useRef<HTMLDivElement>(null);
  const navigate = useProfile(state => state.navigate);
  const colors = useThemeColors();
  useEffect(() => {
    if (!element.current) return;
    const chart = echarts.init(element.current);
    const runs = [...index.profile.roots].sort((a, b) => asNumber(a.t_start_ns) - asNumber(b.t_start_ns) || a.uid.localeCompare(b.uid));
    const assignment = patterns.clusters?.assignment ?? new Map<string, number>();
    const count = Math.max(1, patterns.clusters?.clusters.length ?? 1);
    const outliers = new Set(patterns.anomalies?.runs.map(run => run.root) ?? []);
    const point = (position: number) => {
      const run = runs[position];
      return { value: [position + 1, asNumber(run.duration_ns) / 1e6], uid: run.uid };
    };
    const series = Array.from({ length: count }, (_, cluster) => ({
      name: patterns.clusters?.clusters.length ? shapeName(cluster, count) : 'Runs',
      type: 'scatter',
      symbolSize: 9,
      itemStyle: { color: colors.palette[cluster % colors.palette.length] },
      data: runs.map((run, position) => ({ run, position })).filter(({ run }) => (assignment.get(run.uid) ?? 0) === cluster && !outliers.has(run.uid)).map(({ position }) => point(position)),
    }));
    const markLines = (patterns.changepoints?.points ?? []).map(shift => ({ xAxis: shift.index + 0.5, label: { formatter: `×${(shift.afterNs / Math.max(1, shift.beforeNs)).toFixed(1)}`, color: colors.highlight } }));
    chart.setOption({
      animation: false,
      grid: { left: 60, right: 20, top: 36, bottom: 40 },
      legend: { top: 0, textStyle: { color: colors.muted } },
      tooltip: {
        trigger: 'item',
        formatter: (params: { data: { value: [number, number]; uid: string }; seriesName: string }) =>
          `Run ${params.data.value[0]} · ${params.seriesName}<br/>${params.data.value[1].toFixed(2)} ms<br/><small>${params.data.uid}</small>`,
      },
      xAxis: { type: 'value', name: 'run (start order)', nameLocation: 'middle', nameGap: 26, nameTextStyle: { color: colors.muted }, axisLabel: { color: colors.muted }, splitLine: { show: false } },
      yAxis: { type: 'log', name: 'ms', nameTextStyle: { color: colors.muted }, axisLabel: { color: colors.muted }, splitLine: { lineStyle: { color: colors.line } } },
      series: [
        ...series,
        {
          name: 'Outliers',
          type: 'scatter',
          symbol: 'diamond',
          symbolSize: 15,
          itemStyle: { color: colors.danger },
          data: runs.map((run, position) => ({ run, position })).filter(({ run }) => outliers.has(run.uid)).map(({ position }) => point(position)),
          markLine: markLines.length ? { symbol: 'none', lineStyle: { color: colors.highlight, type: 'dashed', width: 2 }, data: markLines } : undefined,
        },
      ],
    });
    chart.on('click', event => {
      const uid = (event.data as { uid?: string } | null)?.uid;
      if (uid) navigate({ kind: 'root', uid }, 'runs');
    });
    const observer = new ResizeObserver(() => chart.resize());
    observer.observe(element.current);
    return () => {
      observer.disconnect();
      chart.dispose();
    };
  }, [index, patterns, colors, navigate]);
  return <div className="pattern-chart" ref={element} role="img" aria-label="Run durations by shape, outliers and level shifts" />;
}

function GrowthChart({ index, fit }: { index: ProfileIndex; fit: GrowthFit }) {
  const element = useRef<HTMLDivElement>(null);
  const navigate = useProfile(state => state.navigate);
  const colors = useThemeColors();
  useEffect(() => {
    if (!element.current) return;
    const chart = echarts.init(element.current);
    const curve = fit.model ? fittedCurve(fit.points, fit.model) : [];
    chart.setOption({
      animation: false,
      grid: { left: 64, right: 20, top: 20, bottom: 40 },
      tooltip: { trigger: 'item' },
      xAxis: { type: 'value', name: fit.metric, nameLocation: 'middle', nameGap: 26, nameTextStyle: { color: colors.muted }, axisLabel: { color: colors.muted }, splitLine: { show: false } },
      yAxis: { type: 'value', axisLabel: { color: colors.muted, formatter: (value: number) => formatNs(value) }, splitLine: { lineStyle: { color: colors.line } } },
      series: [
        {
          type: 'scatter',
          symbolSize: 7,
          itemStyle: { color: colors.accent },
          data: fit.points.map(([x, y, uid]) => ({ value: [x, y], uid })),
          tooltip: { formatter: (params: { data: { value: [number, number] } }) => `${fit.metric} ${params.data.value[0]}<br/>${formatNs(params.data.value[1])}` },
        },
        ...(curve.length ? [{ type: 'line', showSymbol: false, data: curve, lineStyle: { color: colors.highlight, width: 2 }, tooltip: { show: false } }] : []),
      ],
    });
    chart.on('click', event => {
      const uid = (event.data as { uid?: string } | null)?.uid;
      if (uid) navigate({ kind: 'root', uid }, 'runs');
    });
    const observer = new ResizeObserver(() => chart.resize());
    observer.observe(element.current);
    return () => {
      observer.disconnect();
      chart.dispose();
    };
  }, [index, fit, colors, navigate]);
  return <div className="growth-chart" ref={element} role="img" aria-label={`${fit.metric} growth fit`} />;
}

function NoPatterns() {
  return (
    <section className="panel compare-empty">
      <div className="eyebrow">PATTERN ANALYSIS</div>
      <h2>No pattern analysis in this profile</h2>
      <p>
        Build with the <code>ml</code> feature to cluster runs by shape, flag outliers, detect level shifts over time, fit growth models against root
        metrics, and find tracked objects that change far more than their peers. All of it runs at export, offline and deterministically.
      </p>
    </section>
  );
}

export function Patterns({ index }: { index: ProfileIndex }) {
  const patterns = useMemo(() => parsePatterns(index), [index]);
  const [selectedFit, setSelectedFit] = useState(0);
  if (!patterns) return <NoPatterns />;
  const clusters = patterns.clusters?.clusters ?? [];
  const outliers = patterns.anomalies?.runs ?? [];
  const shifts = patterns.changepoints?.points ?? [];
  const fits = patterns.complexity?.fits ?? [];
  const modelled = fits.filter(fit => fit.model);
  const ordered = [...modelled, ...fits.filter(fit => !fit.model)];
  const fit = ordered[Math.min(selectedFit, Math.max(0, ordered.length - 1))];
  const churn = patterns.churn?.instances ?? [];
  return (
    <div className="panel-stack">
      <div className="stats-grid">
        <StatCard label="Run shapes" value={clusters.length || '—'} detail={patterns.clusters?.silhouette ? `silhouette ${patterns.clusters.silhouette.toFixed(2)}` : (patterns.clusters?.explanation ?? 'not computed')} accent />
        <StatCard label="Outlier runs" value={outliers.length} detail="Robust z within their shape" tone={outliers.length ? 'warn' : ''} />
        <StatCard label="Level shifts" value={shifts.length} detail="Changes in run duration over time" tone={shifts.length ? 'warn' : ''} />
        <StatCard label="Growth models" value={modelled.length} detail="Fits with R² ≥ 0.8" />
      </div>
      <Panel eyebrow="RUN MAP" title="Every run, by shape" aside="Click a point to open the run · dashed lines mark level shifts">
        {index.profile.roots.length ? <RunMap index={index} patterns={patterns} /> : <Empty>No retained runs.</Empty>}
      </Panel>
      <Panel eyebrow="CLUSTERS" title="Shapes of execution" aside={clusters.length ? `${clusters.length} shapes` : undefined}>
        <SectionState section={patterns.clusters}>
          {clusters.length === 0 ? (
            <Empty>Runs do not separate into distinct shapes.</Empty>
          ) : (
            <div className="shape-grid">
              {clusters.map(cluster => (
                <div key={cluster.id} className="shape-card">
                  <div className="shape-head">
                    <i style={{ background: `var(--palette-${cluster.id % 8})` }} />
                    <strong>{shapeName(cluster.id, clusters.length)}</strong>
                    <span>{cluster.size} runs</span>
                  </div>
                  <div className="shape-median">{formatNs(cluster.medianNs)}</div>
                  <small>median run duration</small>
                  <div className="shape-traits">
                    {cluster.distinguishing.map(trait => (
                      <div key={trait.span}>
                        <EntityLink entity={{ kind: 'span', id: trait.span }} />
                        <span className={trait.delta > 0 ? 'warn' : 'good'}>
                          {(trait.share * 100).toFixed(0)}% ({trait.delta >= 0 ? '+' : ''}
                          {(trait.delta * 100).toFixed(0)} pts)
                        </span>
                      </div>
                    ))}
                  </div>
                  <div className="instance-chips compact">
                    {cluster.roots.slice(0, 4).map(uid => (
                      <EntityLink key={uid} entity={{ kind: 'root', uid }} />
                    ))}
                    {cluster.roots.length > 4 && <span className="subtle">+{cluster.size - 4}</span>}
                  </div>
                </div>
              ))}
            </div>
          )}
        </SectionState>
      </Panel>
      <div className="runs-grid">
        <Panel eyebrow="ANOMALIES" title="Runs unlike their shape" aside={`${outliers.length} runs`}>
          <SectionState section={patterns.anomalies}>
            {outliers.length === 0 ? (
              <Empty>No run stands out from its shape.</Empty>
            ) : (
              <div className="pattern-list">
                {outliers.map(run => (
                  <div key={run.root}>
                    <EntityLink entity={{ kind: 'root', uid: run.root }} />
                    <span>
                      {formatNs(run.durationNs)} vs {formatNs(run.typicalNs)}
                    </span>
                    <strong className={run.z > 0 ? 'warn' : 'good'}>z {run.z.toFixed(1)}</strong>
                  </div>
                ))}
              </div>
            )}
          </SectionState>
        </Panel>
        <Panel eyebrow="CHANGEPOINTS" title="When run duration shifted" aside={`${shifts.length} shifts`}>
          <SectionState section={patterns.changepoints}>
            {shifts.length === 0 ? (
              <Empty>Run duration stayed at one level over the capture.</Empty>
            ) : (
              <div className="pattern-list">
                {shifts.map(shift => (
                  <div key={shift.index}>
                    <span>
                      From run {shift.index + 1} <EntityLink entity={{ kind: 'root', uid: shift.root }} />
                    </span>
                    <span>
                      {formatNs(shift.beforeNs)} → {formatNs(shift.afterNs)}
                    </span>
                    <strong className={shift.afterNs > shift.beforeNs ? 'warn' : 'good'}>×{(shift.afterNs / Math.max(1, shift.beforeNs)).toFixed(2)}</strong>
                  </div>
                ))}
              </div>
            )}
          </SectionState>
        </Panel>
      </div>
      <Panel eyebrow="COMPLEXITY" title="How cost grows with input" aside={fits.length ? `${modelled.length} of ${fits.length} fits explained` : undefined}>
        <SectionState section={patterns.complexity}>
          <div className="growth-layout">
            <div className="growth-list">
              {ordered.map((item, position) => (
                <button key={`${item.metric}:${item.span}`} className={`growth-row ${item === fit ? 'selected' : ''}`} onClick={() => setSelectedFit(position)}>
                  <span>
                    <strong>{item.span === null ? 'Run duration' : spanName(index, item.span)}</strong>
                    <small>vs {item.metric}</small>
                  </span>
                  <span className={`growth-model ${item.model === 'O(n²)' ? 'warn' : ''}`}>{item.model ?? 'no fit'}</span>
                  <span>R² {item.r2.toFixed(2)}</span>
                </button>
              ))}
            </div>
            {fit && (
              <div className="growth-detail">
                <GrowthChart index={index} fit={fit} />
                <div className="growth-bars">
                  {fit.fits.map(model => (
                    <div key={model.model}>
                      <span>{model.model}</span>
                      <div className="weight-track blue">
                        <div style={{ width: `${Math.max(0, Math.min(100, model.r2 * 100))}%` }} />
                      </div>
                      <strong>{model.r2.toFixed(2)}</strong>
                    </div>
                  ))}
                </div>
                {fit.span !== null && <EntityLink entity={{ kind: 'span', id: fit.span }}>Open routine →</EntityLink>}
              </div>
            )}
          </div>
        </SectionState>
      </Panel>
      <Panel eyebrow="OBJECT CHURN" title="Instances that change far more than their peers" aside={`${churn.length} instances`}>
        <SectionState section={patterns.churn}>
          {churn.length === 0 ? (
            <Empty>No tracked instance stands out from its type.</Empty>
          ) : (
            <div className="pattern-list">
              {churn.map(item => (
                <div key={item.instance}>
                  <EntityLink entity={{ kind: 'instance', id: item.instance }} />
                  <span>
                    {formatInteger(item.versions)} versions vs typical {item.typical.toFixed(0)}
                  </span>
                  <strong className="warn">z {item.z.toFixed(1)}</strong>
                </div>
              ))}
            </div>
          )}
        </SectionState>
      </Panel>
    </div>
  );
}
