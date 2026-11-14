import { useMemo, useState } from 'react';
import { asNumber, formatBytes, formatInteger, formatNs, type ProfileIndex } from '../model';
import { compareProfiles, relativeDelta, type RoutineMeasurement } from '../model/comparison';
import { EntityLink, Note, Panel, StatCard } from '../ui/components';

function formatDelta(baseline: bigint, candidate: bigint): { text: string; kind: string } {
  const delta = relativeDelta(baseline, candidate);
  if (delta === null) return { text: 'new', kind: 'worse' };
  if (delta === 0) return { text: '0.0%', kind: 'neutral' };
  const percent = delta * 100;
  return { text: `${percent > 0 ? '+' : ''}${percent.toFixed(1)}%`, kind: percent > 0 ? 'worse' : 'better' };
}

function Metric({
  baseline,
  candidate,
  field,
  format,
}: {
  baseline: RoutineMeasurement | null;
  candidate: RoutineMeasurement | null;
  field: 'selfNs' | 'p99Ns' | 'calls' | 'allocBytes';
  format: (value: bigint) => string;
}) {
  const before = baseline?.[field] ?? 0n;
  const after = candidate?.[field] ?? 0n;
  const delta = formatDelta(before, after);
  return (
    <span className="compare-metric">
      <span>
        {format(before)} → {format(after)}
      </span>
      <strong className={delta.kind}>{delta.text}</strong>
    </span>
  );
}

export function Compare({
  baseline,
  candidate,
  baselineName,
  candidateName,
  onOpenBaseline,
}: {
  baseline: ProfileIndex | null;
  candidate: ProfileIndex;
  baselineName: string;
  candidateName: string;
  onOpenBaseline: () => void;
}) {
  const [query, setQuery] = useState('');
  const comparison = useMemo(() => (baseline ? compareProfiles(baseline, candidate) : null), [baseline, candidate]);
  // Candidate routines by name and file, so rows can open the routine view.
  const candidateSpans = useMemo(() => {
    const map = new Map<string, number>();
    for (const span of candidate.profile.spans) map.set(`${span.name}\u0000${span.file}`, asNumber(span.id));
    return map;
  }, [candidate]);
  const needle = query.trim().toLowerCase();
  const rows = useMemo(
    () => comparison?.rows.filter(row => row.name.toLowerCase().includes(needle) || row.file.toLowerCase().includes(needle)).slice(0, 500) ?? [],
    [comparison, needle],
  );
  if (!baseline || !comparison) {
    return (
      <section className="panel compare-empty">
        <div className="eyebrow">PROFILE DIFF</div>
        <h2>Choose a baseline</h2>
        <p>Load an earlier spanscope profile. The current profile remains the candidate, and all comparison work stays in this browser.</p>
        <button className="primary-button" onClick={onOpenBaseline}>
          ↥ &nbsp; Open baseline <span>JSON or GZIP</span>
        </button>
      </section>
    );
  }
  const totalDelta = formatDelta(comparison.baselineSelfNs, comparison.candidateSelfNs);
  return (
    <div className="panel-stack">
      <section className="compare-files">
        <div>
          <small>BASELINE</small>
          <strong>{baselineName}</strong>
          <span>{baseline.profile.meta.git_sha ?? baseline.profile.meta.started_at}</span>
        </div>
        <b>→</b>
        <div>
          <small>CANDIDATE</small>
          <strong>{candidateName}</strong>
          <span>{candidate.profile.meta.git_sha ?? candidate.profile.meta.started_at}</span>
        </div>
        <button className="outline-button" onClick={onOpenBaseline}>
          Replace baseline
        </button>
      </section>
      {comparison.warnings.map(warning => (
        <div className="compare-warning" key={warning}>
          {warning}
        </div>
      ))}
      <div className="stats-grid">
        <StatCard
          label="Total active self time"
          value={`${formatNs(comparison.baselineSelfNs)} → ${formatNs(comparison.candidateSelfNs)}`}
          detail={totalDelta.text}
          tone={`delta-${totalDelta.kind}`}
          accent
        />
        <StatCard label="Observed calls" value={formatInteger(comparison.candidateCalls)} detail={`Baseline ${formatInteger(comparison.baselineCalls)}`} />
        <StatCard label="Potential regressions" value={comparison.regressions} detail="Self time increased by at least 5%" />
        <StatCard label="Routine changes" value={`+${comparison.added} / −${comparison.removed}`} detail={`${comparison.improvements} potential improvements`} />
      </div>
      <Panel eyebrow="ROUTINE DELTAS" title="Baseline → candidate" aside="Sorted by self-time regression" className="compare-panel">
        <div className="invocation-controls">
          <input aria-label="Search comparison" placeholder="Search routine or source file…" value={query} onChange={event => setQuery(event.target.value)} />
        </div>
        <div className="compare-table-head">
          <span>Routine</span>
          <span>Self active</span>
          <span>P99 wall</span>
          <span>Calls</span>
          <span>Allocated</span>
        </div>
        <div className="compare-rows">
          {rows.map(row => {
            const spanId = candidateSpans.get(row.key);
            return (
              <div className="compare-row" key={row.key}>
                <span className="compare-routine">
                  {spanId !== undefined ? (
                    <EntityLink entity={{ kind: 'span', id: spanId }} className="compare-link">
                      {row.name}
                    </EntityLink>
                  ) : (
                    <strong>{row.name}</strong>
                  )}
                  <small>
                    {row.file}:{row.line}
                  </small>
                  {!row.baseline && <em>NEW</em>}
                  {!row.candidate && <em>REMOVED</em>}
                </span>
                <Metric baseline={row.baseline} candidate={row.candidate} field="selfNs" format={formatNs} />
                <Metric baseline={row.baseline} candidate={row.candidate} field="p99Ns" format={formatNs} />
                <Metric baseline={row.baseline} candidate={row.candidate} field="calls" format={formatInteger} />
                <Metric baseline={row.baseline} candidate={row.candidate} field="allocBytes" format={formatBytes} />
              </div>
            );
          })}
        </div>
        {rows.length === 0 && <div className="no-results">No routines match this filter.</div>}
        <Note>
          Routine identity uses the fully qualified name and source file. P99 is reconstructed from mergeable histogram buckets. Aggregate deltas are
          observed values and are not rescaled for sampling. Routine names open the candidate's routine view.
        </Note>
      </Panel>
    </div>
  );
}
