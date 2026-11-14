import { useMemo, useState } from 'react';
import { asNumber, formatBytes, formatInteger, formatNs, label, type ProfileIndex } from '../model';
import { useProfile } from '../store';
import { EntityLink, Note, Panel, StatCard } from '../ui/components';

type Severity = 'critical' | 'warning' | 'info';

function metric(key: string, value: unknown): string | null {
  if (typeof value !== 'number' && typeof value !== 'bigint' && typeof value !== 'string') return null;
  if (typeof value === 'string') return value;
  if (key.endsWith('_ns')) return formatNs(value);
  if (key.endsWith('_bytes')) return formatBytes(value);
  if ((key === 'share' || key === 'rate') && typeof value === 'number') return `${(value * 100).toFixed(1)}%`;
  if ((key === 'ratio' || key === 'coefficient_of_variation') && typeof value === 'number') return `${value.toFixed(1)}×`;
  if (typeof value === 'number' && !Number.isInteger(value)) return value.toFixed(2);
  return formatInteger(value);
}

export function Insights({ index }: { index: ProfileIndex }) {
  const [severity, setSeverity] = useState<Severity | 'all'>('all');
  const navigate = useProfile(state => state.navigate);
  const analysis = index.profile.analysis;
  const insights = useMemo(() => analysis.insights.filter(item => severity === 'all' || item.severity === severity), [analysis.insights, severity]);
  const counts = { critical: 0, warning: 0, info: 0 };
  for (const item of analysis.insights) counts[item.severity] += 1;
  const explanation = analysis.sections.deterministic_diagnostics?.explanation;
  return (
    <div className="panel-stack">
      <div className="stats-grid">
        <StatCard label="Analysis status" value={<span className="insight-status">{analysis.status.replaceAll('_', ' ')}</span>} detail="Deterministic, evidence-backed rules" accent />
        <StatCard label="Critical" value={counts.critical} detail="Highest observed impact" />
        <StatCard label="Warnings" value={counts.warning} detail="Patterns worth investigating" />
        <StatCard label="Information" value={counts.info} detail="Structural observations" />
      </div>
      <Panel
        eyebrow="AUTOMATIC DIAGNOSTICS"
        title="What deserves attention"
        className="insights-panel"
        aside={
          <select aria-label="Filter insight severity" value={severity} onChange={event => setSeverity(event.target.value as Severity | 'all')}>
            <option value="all">All severities</option>
            <option value="critical">Critical</option>
            <option value="warning">Warning</option>
            <option value="info">Information</option>
          </select>
        }
      >
        {explanation && <p className="insight-method">{explanation}</p>}
        <div className="insight-list">
          {insights.map((item, position) => {
            const spans = item.evidence.spans.map(asNumber);
            const source = spans.length ? index.spans.get(spans[0]) : null;
            const metrics = Object.entries(item.data)
              .map(([key, value]) => ({ key, value: metric(key, value) }))
              .filter((entry): entry is { key: string; value: string } => entry.value !== null)
              .slice(0, 5);
            const open = () => spans.length && navigate({ kind: 'span', id: spans[0] }, 'routines');
            return (
              <div
                className={`insight-card severity-${item.severity} ${spans.length ? 'clickable' : ''}`}
                key={`${item.kind}:${position}`}
                role="button"
                tabIndex={spans.length ? 0 : -1}
                aria-disabled={!spans.length}
                onClick={open}
                onKeyDown={event => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    open();
                  }
                }}
              >
                <span className="insight-severity">{item.severity}</span>
                <span className="insight-body">
                  <small>{item.kind}</small>
                  <strong>{item.title}</strong>
                  <p>{item.explanation}</p>
                  {source && (
                    <em>
                      {source.file}:{formatInteger(source.line)}
                    </em>
                  )}
                  <span className="insight-metrics">
                    {metrics.map(entry => (
                      <span key={entry.key}>
                        <small>{label(entry.key)}</small>
                        <b>{entry.value}</b>
                      </span>
                    ))}
                  </span>
                  <span className="insight-evidence">
                    {spans.map(id => (
                      <EntityLink key={`s${id}`} entity={{ kind: 'span', id }} />
                    ))}
                    {item.evidence.chains.map(id => (
                      <EntityLink key={`c${id}`} entity={{ kind: 'chain', id: asNumber(id) }} />
                    ))}
                    {item.evidence.roots.map(uid => (
                      <EntityLink key={`r${uid}`} entity={{ kind: 'root', uid }} />
                    ))}
                  </span>
                </span>
                <span className="root-arrow">↗</span>
              </div>
            );
          })}
        </div>
        {insights.length === 0 && (
          <div className="analysis-empty">
            {analysis.status === 'insufficient_data'
              ? 'The capture does not contain enough aggregate evidence for diagnostics.'
              : 'No insights match this severity filter.'}
          </div>
        )}
        <Note>These findings describe observed instrumented data. They do not prove a defect, and severity is prioritization rather than probability.</Note>
      </Panel>
    </div>
  );
}
