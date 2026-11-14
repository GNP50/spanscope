import { useMemo, useState } from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { asNumber, formatNs, instanceName, rootName, shortName, type ProfileIndex } from '../model';
import { VIEWS, type Entity } from '../nav/views';
import { useProfile, type Selection } from '../store';
import { EntityLink } from '../ui/components';

function readBootstrap(): { mode: string; suggested?: string } {
  const text = document.getElementById('spanscope-bootstrap')?.textContent ?? '{}';
  try {
    return JSON.parse(text);
  } catch {
    return { mode: 'empty' };
  }
}
const bootstrap = readBootstrap();

function IconButton({ children, label, onClick }: { children: React.ReactNode; label: string; onClick: () => void }) {
  return (
    <button className="icon-button" aria-label={label} title={label} onClick={onClick}>
      {children}
    </button>
  );
}

export function TopBar({
  onOpen,
  onCommands,
  onFollow,
  following,
}: {
  onOpen: () => void;
  onCommands: () => void;
  onFollow?: () => void;
  following: boolean;
}) {
  const filename = useProfile(state => state.filename);
  const index = useProfile(state => state.index);
  const theme = useProfile(state => state.theme);
  const setTheme = useProfile(state => state.setTheme);
  return (
    <header className="topbar">
      <div className="brand">
        <span className="brand-mark">▤</span>
        <span>
          span<span className="brand-accent">scope</span>
        </span>
        <span className="brand-badge">BETA</span>
      </div>
      <div className="topbar-file">
        <span className="live-dot" />
        {index ? (
          <>
            <span className="truncate">{filename}</span>
            <span className="file-caption">schema v{index.profile.schema_version}</span>
          </>
        ) : (
          'No profile loaded'
        )}
      </div>
      <div className="topbar-actions">
        <IconButton label="Open profile" onClick={onOpen}>
          ↥
        </IconButton>
        {onFollow && (
          <button className={`live-follow ${following ? 'active' : ''}`} onClick={onFollow} title="Watch a profile file and reload it when it changes">
            <span />
            {following ? 'Following' : 'Live follow'}
          </button>
        )}
        <IconButton label="Toggle theme" onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}>
          {theme === 'dark' ? '☼' : '☾'}
        </IconButton>
        <button className="command-trigger" onClick={onCommands}>
          <span>⌕</span> Command <kbd>⌘ K</kbd>
        </button>
      </div>
    </header>
  );
}

export function Sidebar() {
  const view = useProfile(state => state.view);
  const setView = useProfile(state => state.setView);
  const index = useProfile(state => state.index);
  const stats: [string, number | string][] = [
    ['Spans', index?.profile.spans.length ?? '—'],
    ['Chains', index?.profile.chains.length ?? '—'],
    ['Roots', index?.profile.roots.length ?? '—'],
    ['Objects', index ? (index.objects?.instances.size ?? 0) : '—'],
  ];
  return (
    <aside className="sidebar">
      <div className="side-label">WORKSPACE</div>
      <nav aria-label="Views">
        {VIEWS.map(item => (
          <button key={item.id} className={`nav-item ${view === item.id ? 'active' : ''}`} onClick={() => setView(item.id)}>
            <span className="nav-icon">{item.icon}</span>
            {item.label}
          </button>
        ))}
      </nav>
      <div className="side-bottom">
        <div className="side-label">CAPTURE</div>
        {stats.map(([label, value]) => (
          <div className="side-stat" key={label}>
            <span>{label}</span>
            <strong>{value}</strong>
          </div>
        ))}
        <div className="side-version">spanscope 0.1 · offline report</div>
      </div>
    </aside>
  );
}

export function EmptyState({ onOpen, error, progress }: { onOpen: () => void; error: string; progress: number | null }) {
  return (
    <div className="empty-stage">
      <div className="empty-grid" />
      <div className="empty-content">
        <div className="eyebrow">
          <span className="pulse-dot" /> PROFILE WORKSPACE
        </div>
        <h1>
          See where your
          <br />
          <em>execution goes.</em>
        </h1>
        <p>Drop in a spanscope profile to explore call chains, root runs, tracked objects and the work beneath every span. Your data stays in this browser.</p>
        <button className="primary-button" onClick={onOpen}>
          ↥ &nbsp; Open profile <span>JSON or GZIP</span>
        </button>
        {bootstrap.mode === 'picker' && (
          <p className="hint">
            Large profile ready nearby: choose <strong>{bootstrap.suggested ?? 'the sidecar file'}</strong> to load it.
          </p>
        )}
        {error && (
          <div className="error-box" role="alert">
            {error}
          </div>
        )}
        {progress !== null && (
          <div className="progress">
            <div style={{ width: `${progress * 100}%` }} />
          </div>
        )}
      </div>
      <div className="empty-art" aria-hidden="true">
        <div className="art-line a" />
        <div className="art-line b" />
        <div className="art-line c" />
        <div className="art-line d" />
        <div className="art-line e" />
        <span className="art-label">root::request</span>
        <span className="art-label second">db::query</span>
      </div>
    </div>
  );
}

/** Text filter, run scope, and one removable chip per selected entity. */
export function FilterBar({ index, showFilters }: { index: ProfileIndex; showFilters: boolean }) {
  const query = useProfile(state => state.query);
  const setQuery = useProfile(state => state.setQuery);
  const rootUid = useProfile(state => state.rootUid);
  const navigate = useProfile(state => state.navigate);
  const clear = useProfile(state => state.clear);
  const reset = useProfile(state => state.reset);
  const spanId = useProfile(state => state.spanId);
  const chainId = useProfile(state => state.chainId);
  const typeId = useProfile(state => state.typeId);
  const instanceId = useProfile(state => state.instanceId);
  const chips: { key: keyof Selection; entity: Entity; label: string }[] = [];
  if (spanId !== null) chips.push({ key: 'spanId', entity: { kind: 'span', id: spanId }, label: 'Span' });
  if (chainId !== null) chips.push({ key: 'chainId', entity: { kind: 'chain', id: chainId }, label: 'Chain' });
  if (typeId !== null) chips.push({ key: 'typeId', entity: { kind: 'type', id: typeId }, label: 'Type' });
  if (instanceId !== null) chips.push({ key: 'instanceId', entity: { kind: 'instance', id: instanceId }, label: 'Instance' });
  const any = query || rootUid || chips.length > 0;
  return (
    <div className="filterbar">
      {showFilters && (
        <>
          <div className="search-wrap">
            <span>⌕</span>
            <input aria-label="Search spans" placeholder="Filter by span name…" value={query} onChange={event => setQuery(event.target.value)} />
          </div>
          <select
            aria-label="Filter by root"
            value={rootUid ?? ''}
            onChange={event => (event.target.value ? navigate({ kind: 'root', uid: event.target.value }, 'stay') : clear('rootUid'))}
          >
            <option value="">All roots</option>
            {index.profile.roots.map(root => (
              <option key={root.uid} value={root.uid}>
                {root.uid.slice(0, 12)} · {formatNs(root.duration_ns)}
              </option>
            ))}
          </select>
        </>
      )}
      {!showFilters && rootUid && (
        <span className="selection-chip">
          Run
          <EntityLink entity={{ kind: 'root', uid: rootUid }} />
          <button aria-label="Clear run" onClick={() => clear('rootUid')}>
            ×
          </button>
        </span>
      )}
      {chips.map(chip => (
        <span className="selection-chip" key={chip.key}>
          {chip.label}
          <EntityLink entity={chip.entity} />
          <button aria-label={`Clear ${chip.label.toLowerCase()}`} onClick={() => clear(chip.key)}>
            ×
          </button>
        </span>
      ))}
      {any && (
        <button className="reset-button" onClick={reset}>
          Clear filters
        </button>
      )}
    </div>
  );
}

interface Command {
  label: string;
  hint: string;
  group: string;
  run: () => void;
}

/** Views, actions, and every named entity in the profile. */
export function CommandPalette({ open, setOpen, onOpen }: { open: boolean; setOpen: (value: boolean) => void; onOpen: () => void }) {
  const [query, setQuery] = useState('');
  const index = useProfile(state => state.index);
  const setView = useProfile(state => state.setView);
  const navigate = useProfile(state => state.navigate);
  const reset = useProfile(state => state.reset);
  const entities = useMemo<Command[]>(() => {
    if (!index) return [];
    const list: Command[] = [];
    for (const span of index.profile.spans) {
      list.push({ label: span.name, hint: '◉', group: 'Routine', run: () => navigate({ kind: 'span', id: asNumber(span.id) }) });
    }
    index.profile.roots.forEach((root, position) =>
      list.push({ label: `Run #${position + 1} ${shortName(rootName(index, root))} ${root.uid}`, hint: '◇', group: 'Run', run: () => navigate({ kind: 'root', uid: root.uid }) }),
    );
    for (const [id, type] of index.objects?.types ?? []) {
      list.push({ label: type.name, hint: '◈', group: 'Type', run: () => navigate({ kind: 'type', id }) });
    }
    for (const [id, instance] of index.objects?.instances ?? []) {
      list.push({ label: instanceName(index, instance), hint: '●', group: 'Instance', run: () => navigate({ kind: 'instance', id }) });
    }
    return list;
  }, [index, navigate]);
  const needle = query.trim().toLowerCase();
  const commands: Command[] = [
    ...VIEWS.map(item => ({ label: `Go to ${item.label}`, hint: item.icon, group: 'View', run: () => setView(item.id) })),
    { label: 'Open a profile', hint: '↥', group: 'Action', run: onOpen },
    { label: 'Clear filters', hint: '×', group: 'Action', run: reset },
  ].filter(item => item.label.toLowerCase().includes(needle));
  const matches = needle ? entities.filter(item => item.label.toLowerCase().includes(needle)).slice(0, 40) : [];
  const close = () => {
    setOpen(false);
    setQuery('');
  };
  return (
    <Dialog.Root open={open} onOpenChange={value => (value ? setOpen(true) : close())}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog-content" aria-describedby={undefined}>
          <Dialog.Title>Command palette</Dialog.Title>
          <input autoFocus placeholder="Jump to a view, routine, run, type or instance…" value={query} onChange={event => setQuery(event.target.value)} />
          <div className="command-list">
            {[...commands, ...matches].map(item => (
              <button
                key={`${item.group}:${item.label}`}
                onClick={() => {
                  item.run();
                  close();
                }}
              >
                <span>{item.hint}</span>
                {item.label}
                <kbd>{item.group}</kbd>
              </button>
            ))}
          </div>
          <p>ESC to close · Ctrl / ⌘ + K to open · search names of routines, runs, types and instances</p>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

