import { useMemo, useState } from 'react';
import { asNumber, formatInteger, formatNs, type ProfileIndex } from './core';
import { useProfile } from './store';

type Severity = 'critical' | 'warning' | 'info';

function label(value: string): string {
  return value.replaceAll('_', ' ').replace(/^./, character => character.toUpperCase());
}

function metric(key: string, value: unknown): string | null {
  if (typeof value !== 'number' && typeof value !== 'bigint' && typeof value !== 'string') return null;
  if (key.endsWith('_ns') && (typeof value === 'number' || typeof value === 'bigint')) return formatNs(value);
  if ((key.endsWith('_bytes') || key === 'alloc_bytes') && (typeof value === 'number' || typeof value === 'bigint')) return `${formatInteger(value)} B`;
  if ((key === 'share' || key === 'rate') && typeof value === 'number') return `${(value * 100).toFixed(1)}%`;
  if ((key === 'ratio' || key === 'coefficient_of_variation') && typeof value === 'number') return `${value.toFixed(1)}×`;
  if (typeof value === 'number' || typeof value === 'bigint') return formatInteger(value);
  return value;
}

export function InsightsView({ index }: { index: ProfileIndex }) {
  const [severity, setSeverity] = useState<Severity | 'all'>('all');
  const selectSpan = useProfile(state => state.selectSpan);
  const setView = useProfile(state => state.setView);
  const analysis = index.profile.analysis;
  const insights = useMemo(() => analysis.insights.filter(item => severity === 'all' || item.severity === severity), [analysis.insights, severity]);
  const counts = { critical: 0, warning: 0, info: 0 };
  for (const item of analysis.insights) counts[item.severity] += 1;
  const explanation = analysis.sections.deterministic_diagnostics?.explanation;
  return <div className="panel-stack">
    <div className="stats-grid"><div className="stat-card accent"><div className="stat-label">Analysis status</div><div className="stat-value insight-status">{analysis.status.replaceAll('_', ' ')}</div><div className="stat-detail">Deterministic, evidence-backed rules</div></div><div className="stat-card"><div className="stat-label">Critical</div><div className="stat-value">{counts.critical}</div><div className="stat-detail">Highest observed impact</div></div><div className="stat-card"><div className="stat-label">Warnings</div><div className="stat-value">{counts.warning}</div><div className="stat-detail">Patterns worth investigating</div></div><div className="stat-card"><div className="stat-label">Information</div><div className="stat-value">{counts.info}</div><div className="stat-detail">Structural observations</div></div></div>
    <section className="panel insights-panel"><div className="panel-heading"><div><div className="eyebrow">AUTOMATIC DIAGNOSTICS</div><h2>What deserves attention</h2></div><select aria-label="Filter insight severity" value={severity} onChange={event => setSeverity(event.target.value as Severity | 'all')}><option value="all">All severities</option><option value="critical">Critical</option><option value="warning">Warning</option><option value="info">Information</option></select></div>{explanation && <p className="insight-method">{explanation}</p>}<div className="insight-list">{insights.map((item, position) => {
      const spanId = item.evidence.spans.length ? asNumber(item.evidence.spans[0]) : null;
      const source = spanId === null ? null : index.spans.get(spanId);
      const metrics = Object.entries(item.data).map(([key, value]) => ({ key, value: metric(key, value) })).filter((entry): entry is { key: string; value: string } => entry.value !== null).slice(0, 5);
      return <button className={`insight-card severity-${item.severity}`} key={`${item.kind}:${position}`} onClick={() => { if (spanId !== null) { selectSpan(spanId); setView('routines'); } }} disabled={spanId === null}><span className="insight-severity">{item.severity}</span><span className="insight-body"><small>{item.kind}</small><strong>{item.title}</strong><p>{item.explanation}</p>{source && <em>{source.file}:{formatInteger(source.line)}</em>}<span className="insight-metrics">{metrics.map(entry => <span key={entry.key}><small>{label(entry.key)}</small><b>{entry.value}</b></span>)}</span></span><span className="root-arrow">↗</span></button>;
    })}</div>{insights.length === 0 && <div className="analysis-empty">{analysis.status === 'insufficient_data' ? 'The capture does not contain enough aggregate evidence for diagnostics.' : 'No insights match this severity filter.'}</div>}<p className="panel-note">These findings describe observed instrumented data. They do not prove a defect, and severity is prioritization rather than probability.</p></section>
  </div>;
}
