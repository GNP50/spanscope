import { create } from 'zustand';
import type { ProfileIndex } from './model';
import { EMPTY, hashFromLocation, locationFromHash, selectionFor, type Location, type Selection } from './nav/location';
import { defaultView, type Entity, type View } from './nav/views';

export type { Selection };
export type Theme = 'dark' | 'light';

interface State extends Location {
  index: ProfileIndex | null;
  filename: string;
  baselineIndex: ProfileIndex | null;
  baselineFilename: string;
  theme: Theme;
  setIndex: (index: ProfileIndex, filename: string) => void;
  setBaseline: (index: ProfileIndex, filename: string) => void;
  setTheme: (theme: Theme) => void;
  /** Typing in a filter: replaces the history entry instead of adding one. */
  setQuery: (query: string) => void;
  setView: (view: View) => void;
  /** Select an entity and optionally switch view; adds a history entry. */
  navigate: (entity: Entity, view?: View | 'stay') => void;
  /** Clear one part of the selection. */
  clear: (key: keyof Selection) => void;
  reset: () => void;
}

function initialTheme(): Theme {
  try {
    const saved = localStorage.getItem('spanscope-theme');
    if (saved === 'dark' || saved === 'light') return saved;
  } catch {
    // Storage can be unavailable on file:// or in private windows.
  }
  return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

/** How the next state change is written to history. */
let historyMode: 'push' | 'replace' | 'none' = 'replace';

export const useProfile = create<State>((set, get) => ({
  index: null,
  filename: '',
  baselineIndex: null,
  baselineFilename: '',
  theme: initialTheme(),
  ...locationFromHash(location.hash),
  setIndex: (index, filename) => set({ index, filename }),
  setBaseline: (baselineIndex, baselineFilename) => {
    historyMode = 'push';
    set({ baselineIndex, baselineFilename, view: 'compare' });
  },
  setTheme: theme => {
    try {
      localStorage.setItem('spanscope-theme', theme);
    } catch {
      // Not persisted; the theme still applies to this page.
    }
    set({ theme });
  },
  setQuery: query => {
    historyMode = 'replace';
    set({ query });
  },
  setView: view => {
    historyMode = 'push';
    set({ view });
  },
  navigate: (entity, view) => {
    historyMode = 'push';
    const target = view === 'stay' ? get().view : (view ?? defaultView[entity.kind]);
    set({ ...selectionFor(entity, get().index), view: target });
  },
  clear: key => {
    historyMode = 'push';
    set(key === 'spanId' ? { spanId: null, chainId: null } : key === 'typeId' ? { typeId: null, instanceId: null } : { [key]: null });
  },
  reset: () => {
    historyMode = 'push';
    set({ ...EMPTY, query: '' });
  },
}));

useProfile.subscribe(state => {
  const hash = hashFromLocation(state);
  if (historyMode !== 'none' && location.hash !== hash) {
    if (historyMode === 'push') history.pushState(null, '', hash);
    else history.replaceState(null, '', hash);
  }
  historyMode = 'replace';
  document.documentElement.dataset.theme = state.theme;
});

window.addEventListener('popstate', () => {
  historyMode = 'none';
  useProfile.setState(locationFromHash(location.hash));
});

document.documentElement.dataset.theme = useProfile.getState().theme;
