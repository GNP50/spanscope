import { useCallback, useMemo, useRef, useState } from 'react';
import { CommandPalette, EmptyState, FilterBar, Sidebar, TopBar } from './app/Chrome';
import { useFollow, useGlobalInput, useLoader } from './app/hooks';
import { filterChains, type ExactChain, type ProfileIndex } from './model';
import { viewMeta, type View } from './nav/views';
import { useProfile } from './store';
import { ErrorBoundary } from './ui/ErrorBoundary';
import { Chains } from './views/Chains';
import { Compare } from './views/Compare';
import { Dependencies } from './views/Dependencies';
import { Flame } from './views/Flame';
import { Insights } from './views/Insights';
import { Objects } from './views/Objects';
import { Overview } from './views/Overview';
import { Patterns } from './views/Patterns';
import { Raw } from './views/Raw';
import { Routines } from './views/Routines';
import { Runs } from './views/Runs';
import { Timeline } from './views/Timeline';

const PROFILE_TYPES = '.json,.gz,.json.gz,application/json,application/gzip';

function CurrentView({ view, index, chains, openBaseline }: { view: View; index: ProfileIndex; chains: ExactChain[]; openBaseline: () => void }) {
  const filename = useProfile(state => state.filename);
  const baselineIndex = useProfile(state => state.baselineIndex);
  const baselineFilename = useProfile(state => state.baselineFilename);
  switch (view) {
    case 'overview':
      return <Overview index={index} chains={chains} />;
    case 'compare':
      return <Compare baseline={baselineIndex} candidate={index} baselineName={baselineFilename} candidateName={filename} onOpenBaseline={openBaseline} />;
    case 'insights':
      return <Insights index={index} />;
    case 'patterns':
      return <Patterns index={index} />;
    case 'timeline':
      return <Timeline index={index} />;
    case 'routines':
      return <Routines index={index} />;
    case 'runs':
      return <Runs index={index} />;
    case 'dependencies':
      return <Dependencies index={index} />;
    case 'objects':
      return <Objects index={index} />;
    case 'flame':
      return <Flame index={index} chains={chains} />;
    case 'chains':
      return <Chains index={index} chains={chains} />;
    case 'raw':
      return <Raw index={index} />;
  }
}

export default function App() {
  const fileInput = useRef<HTMLInputElement>(null);
  const baselineInput = useRef<HTMLInputElement>(null);
  const [commands, setCommands] = useState(false);
  const { load, error, progress } = useLoader();
  const follow = useFollow(load);
  const toggleCommands = useCallback(() => setCommands(value => !value), []);
  useGlobalInput(load, toggleCommands);
  const index = useProfile(state => state.index);
  const view = useProfile(state => state.view);
  const query = useProfile(state => state.query);
  const spanId = useProfile(state => state.spanId);
  const rootUid = useProfile(state => state.rootUid);
  const chains = useMemo(() => (index ? filterChains(index, { query, spanId, rootUid }) : []), [index, query, spanId, rootUid]);
  const open = useCallback(() => fileInput.current?.click(), []);
  const openBaseline = useCallback(() => baselineInput.current?.click(), []);
  const meta = viewMeta(view);
  return (
    <div className="app-shell">
      <TopBar onOpen={open} onCommands={() => setCommands(true)} onFollow={follow.available ? () => void follow.toggle() : undefined} following={follow.following} />
      <div className="shell-body">
        <Sidebar />
        <main className="main-content">
          {!index ? (
            <EmptyState onOpen={open} error={error} progress={progress} />
          ) : (
            <>
              <div className="page-header">
                <div>
                  <div className="eyebrow">EXECUTION PROFILE / {view.toUpperCase()}</div>
                  <h1>{meta.label}</h1>
                  <p>
                    {index.profile.meta.program} · {index.profile.meta.started_at}
                  </p>
                </div>
                <div className="page-actions">
                  <button className="outline-button" onClick={() => history.back()} title="Previous selection (browser Back)">
                    ← Back
                  </button>
                  <button className="outline-button" onClick={open}>
                    ↥ &nbsp; New profile
                  </button>
                </div>
              </div>
              <FilterBar index={index} showFilters={meta.filters} />
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
              <ErrorBoundary resetKey={view}>
                <CurrentView view={view} index={index} chains={chains} openBaseline={openBaseline} />
              </ErrorBoundary>
            </>
          )}
        </main>
      </div>
      <input
        ref={fileInput}
        hidden
        type="file"
        accept={PROFILE_TYPES}
        onChange={event => {
          const file = event.target.files?.[0];
          if (file) {
            follow.stop();
            load(file, 'candidate');
          }
          event.currentTarget.value = '';
        }}
      />
      <input
        ref={baselineInput}
        hidden
        type="file"
        accept={PROFILE_TYPES}
        onChange={event => {
          const file = event.target.files?.[0];
          if (file) load(file, 'baseline');
          event.currentTarget.value = '';
        }}
      />
      <CommandPalette open={commands} setOpen={setCommands} onOpen={open} />
    </div>
  );
}
