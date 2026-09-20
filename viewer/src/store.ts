import { create } from 'zustand';
import type { ProfileIndex } from './core';

export type View = 'overview' | 'routines' | 'runs' | 'dependencies' | 'flame' | 'chains' | 'raw';
interface State {
  index: ProfileIndex | null;
  filename: string;
  view: View;
  theme: 'dark' | 'light';
  query: string;
  spanId: number | null;
  chainId: number | null;
  rootUid: string | null;
  setIndex: (index: ProfileIndex, filename: string) => void;
  setView: (view: View) => void;
  setTheme: (theme: 'dark' | 'light') => void;
  setQuery: (query: string) => void;
  selectSpan: (id: number | null) => void;
  selectChain: (id: number | null, spanId: number | null) => void;
  selectRoot: (uid: string | null) => void;
  reset: () => void;
}

function fromHash() {
  const params = new URLSearchParams(location.hash.replace(/^#/, ''));
  const candidate = params.get('view');
  const view: View = candidate === 'routines' || candidate === 'runs' || candidate === 'dependencies' || candidate === 'flame' || candidate === 'chains' || candidate === 'raw' ? candidate : 'overview';
  const span = params.get('span');
  return {
    view,
    spanId: span !== null && /^\d+$/.test(span) ? Number(span) : null,
    rootUid: params.get('root'),
    query: params.get('q') ?? '',
  };
}

const initial = fromHash();
export const useProfile = create<State>((set) => ({
  index: null, filename: '', view: initial.view, theme: 'dark',
  query: initial.query, spanId: initial.spanId, chainId: null, rootUid: initial.rootUid,
  setIndex: (index, filename) => set({ index, filename }),
  setView: view => set({ view }),
  setTheme: theme => set({ theme }),
  setQuery: query => set({ query }),
  selectSpan: spanId => set({ spanId, chainId: null }),
  selectChain: (chainId, spanId) => set({ chainId, spanId }),
  selectRoot: rootUid => set({ rootUid, chainId: null }),
  reset: () => set({ query: '', spanId: null, chainId: null, rootUid: null }),
}));

useProfile.subscribe(state => {
  const params = new URLSearchParams();
  params.set('view', state.view);
  if (state.spanId !== null) params.set('span', String(state.spanId));
  if (state.rootUid) params.set('root', state.rootUid);
  if (state.query) params.set('q', state.query);
  const hash = `#${params}`;
  if (location.hash !== hash) history.replaceState(null, '', hash);
  document.documentElement.dataset.theme = state.theme;
});
