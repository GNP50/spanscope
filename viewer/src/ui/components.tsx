import { useState, type ReactNode } from 'react';
import { asNumber, formatNs, ratio, rootName, shortName, spanName, type ExactRoot, type ProfileIndex } from '../model';
import { useProfile } from '../store';
import type { Entity, View } from '../nav/views';

/** Standard panel with the eyebrow / title / aside heading used across views. */
export function Panel({
  eyebrow,
  title,
  aside,
  className = '',
  children,
}: {
  eyebrow: string;
  title: ReactNode;
  aside?: ReactNode;
  className?: string;
  children?: ReactNode;
}) {
  return (
    <section className={`panel ${className}`}>
      <div className="panel-heading">
        <div>
          <div className="eyebrow">{eyebrow}</div>
          <h2>{title}</h2>
        </div>
        {aside !== undefined && (typeof aside === 'string' ? <span className="subtle">{aside}</span> : aside)}
      </div>
      {children}
    </section>
  );
}

export function StatCard({ label, value, detail, accent, tone }: { label: string; value: ReactNode; detail: ReactNode; accent?: boolean; tone?: string }) {
  return (
    <div className={`stat-card ${accent ? 'accent' : ''}`}>
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      <div className={`stat-detail ${tone ?? ''}`}>{detail}</div>
    </div>
  );
}

export function Note({ children }: { children: ReactNode }) {
  return <p className="panel-note">{children}</p>;
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="analysis-empty">{children}</div>;
}

export function WeightBar({ value, max, tone = 'mint' }: { value: bigint; max: bigint; tone?: 'mint' | 'blue' | 'warm' }) {
  return (
    <div className={`weight-track ${tone}`} aria-hidden="true">
      <div style={{ width: `${ratio(value, max)}%` }} />
    </div>
  );
}

export function SourceLocation({ file, line }: { file?: string; line?: number | bigint }) {
  const [copied, setCopied] = useState(false);
  if (!file) return null;
  const location = `${file}${line === undefined ? '' : `:${line}`}`;
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(location);
    } catch {
      const input = document.createElement('textarea');
      input.value = location;
      input.style.position = 'fixed';
      input.style.opacity = '0';
      document.body.appendChild(input);
      input.select();
      document.execCommand('copy');
      input.remove();
    }
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  };
  return (
    <button className="source-location" onClick={() => void copy()} title="Copy file and line for your editor">
      {location} <span>{copied ? 'Copied' : 'Copy'}</span>
    </button>
  );
}

function entityLabel(index: ProfileIndex, entity: Entity): string {
  switch (entity.kind) {
    case 'span':
      return spanName(index, entity.id);
    case 'chain':
      return `Chain #${entity.id}`;
    case 'root': {
      const root = index.roots.get(entity.uid);
      return root ? `${shortName(rootName(index, root))} · ${entity.uid.slice(0, 8)}` : entity.uid;
    }
    case 'type':
      return shortName(index.objects?.types.get(entity.id)?.name ?? `Type #${entity.id}`);
    case 'instance': {
      const instance = index.objects?.instances.get(entity.id);
      if (!instance) return `Instance #${entity.id}`;
      return `${shortName(index.objects?.types.get(asNumber(instance.type_id))?.name ?? '?')} ${instance.key}`;
    }
  }
}

const ENTITY_ICON: Record<Entity['kind'], string> = { span: '◉', chain: '≡', root: '◇', type: '◈', instance: '●' };

/** A chip that selects an entity and opens a view; Back returns here. */
export function EntityLink({
  entity,
  view,
  children,
  className = '',
  title,
}: {
  entity: Entity;
  view?: View | 'stay';
  children?: ReactNode;
  className?: string;
  title?: string;
}) {
  const index = useProfile(state => state.index);
  const navigate = useProfile(state => state.navigate);
  const text = index ? entityLabel(index, entity) : '';
  return (
    <button
      className={`entity-link entity-${entity.kind} ${className}`}
      title={title ?? text}
      onClick={event => {
        event.stopPropagation();
        navigate(entity, view);
      }}
    >
      <span aria-hidden="true">{ENTITY_ICON[entity.kind]}</span>
      {children ?? text}
    </button>
  );
}

/** Small row of links to other views for the current selection. */
export function ViewLinks({ entity, views }: { entity: Entity; views: { view: View; label: string }[] }) {
  return (
    <div className="view-links">
      {views.map(item => (
        <EntityLink key={item.view} entity={entity} view={item.view} className="view-link">
          {item.label} →
        </EntityLink>
      ))}
    </div>
  );
}

/** Retained-run chooser shared by Runs and Timeline. */
export function RunPicker({ index, selected }: { index: ProfileIndex; selected: ExactRoot | null }) {
  const navigate = useProfile(state => state.navigate);
  return (
    <div className="run-picker">
      {index.profile.roots.map((root, position) => (
        <button
          key={root.uid}
          className={selected?.uid === root.uid ? 'selected' : ''}
          onClick={() => navigate({ kind: 'root', uid: root.uid }, 'stay')}
        >
          <span>
            #{position + 1} · {shortName(rootName(index, root))}
            <small>{root.uid}</small>
          </span>
          <strong>{formatNs(root.duration_ns)}</strong>
        </button>
      ))}
    </div>
  );
}
