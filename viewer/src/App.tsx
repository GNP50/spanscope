import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { createColumnHelper, flexRender, getCoreRowModel, getSortedRowModel, useReactTable, type SortingState } from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import * as echarts from 'echarts/core';
import { BarChart } from 'echarts/charts';
import { GridComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import ProfileWorker from './worker?worker&inline';
import { asNumber, buildFlame, exactJson, filterChains, formatInteger, formatNs, type ExactChain, type ProfileIndex } from './core';
import { useProfile, type View } from './store';
import { DependenciesView, RoutinesView, RunsView } from './AnalysisViews';

echarts.use([BarChart, GridComponent, TooltipComponent, CanvasRenderer]);

const NAV: { id: View; label: string; icon: string }[] = [
  { id: 'overview', label: 'Overview', icon: '◫' },
  { id: 'routines', label: 'Routines', icon: '◉' },
  { id: 'runs', label: 'Runs & metrics', icon: '▦' },
  { id: 'dependencies', label: 'Dependencies', icon: '⌁' },
  { id: 'flame', label: 'Flame graph', icon: '▥' },
  { id: 'chains', label: 'Call chains', icon: '≡' },
  { id: 'raw', label: 'Raw explorer', icon: '{}' },
];

function readBootstrap(): { mode: string; suggested?: string } {
  const text = document.getElementById('spanscope-bootstrap')?.textContent ?? '{}';
  try { return JSON.parse(text); } catch { return { mode: 'empty' }; }
}
const bootstrap = readBootstrap();

async function readProfileFile(file: File): Promise<string> {
  const magic = new Uint8Array(await file.slice(0, 2).arrayBuffer());
  if (magic[0] === 0x1f && magic[1] === 0x8b) {
    if (typeof DecompressionStream === 'undefined') throw new Error('This browser cannot decompress gzip files.');
    return new Response(file.stream().pipeThrough(new DecompressionStream('gzip'))).text();
  }
  return file.text();
}

function useLoader() {
  const [error, setError] = useState('');
  const [progress, setProgress] = useState<number | null>(null);
  const workerRef = useRef<Worker | null>(null);
  const requestRef = useRef(0);
  const setIndex = useProfile(state => state.setIndex);
  useEffect(() => {
    const worker = new ProfileWorker();
    workerRef.current = worker;
    worker.onmessage = (event: MessageEvent<{ id: number; type: string; value?: number; index?: ProfileIndex; message?: string }>) => {
      if (event.data.id !== requestRef.current) return;
      if (event.data.type === 'progress') setProgress(event.data.value ?? 0);
      else if (event.data.type === 'ready' && event.data.index) { setIndex(event.data.index, pendingName.current); setProgress(null); setError(''); }
      else if (event.data.type === 'error') { setError(event.data.message ?? 'Could not read profile.'); setProgress(null); }
    };
    const inline = document.getElementById('spanscope-profile')?.textContent;
    if (inline) { requestRef.current += 1; pendingName.current = 'Embedded profile'; worker.postMessage({ id: requestRef.current, text: inline }); setProgress(0); }
    return () => { worker.terminate(); workerRef.current = null; };
  }, [setIndex]);
  const pendingName = useRef('');
  const load = useCallback(async (file: File) => {
    const id = ++requestRef.current;
    pendingName.current = file.name;
    setError(''); setProgress(0);
    try {
      const text = await readProfileFile(file);
      if (id !== requestRef.current) return;
      workerRef.current?.postMessage({ id, text });
    } catch (failure) { setError(failure instanceof Error ? failure.message : String(failure)); setProgress(null); }
  }, []);
  return { load, error, progress };
}

function IconButton({ children, label, onClick }: { children: React.ReactNode; label: string; onClick: () => void }) {
  return <button className="icon-button" aria-label={label} title={label} onClick={onClick}>{children}</button>;
}

function TopBar({ onOpen, onCommands }: { onOpen: () => void; onCommands: () => void }) {
  const filename = useProfile(s => s.filename);
  const index = useProfile(s => s.index);
  const theme = useProfile(s => s.theme);
  const setTheme = useProfile(s => s.setTheme);
  return <header className="topbar">
    <div className="brand"><span className="brand-mark">▤</span><span>span<span className="brand-accent">scope</span></span><span className="brand-badge">BETA</span></div>
    <div className="topbar-file"><span className="live-dot" />{index ? <><span className="truncate">{filename}</span><span className="file-caption">schema v{index.profile.schema_version}</span></> : 'No profile loaded'}</div>
    <div className="topbar-actions"><IconButton label="Open profile" onClick={onOpen}>↥</IconButton><IconButton label="Toggle theme" onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}>{theme === 'dark' ? '☼' : '☾'}</IconButton><button className="command-trigger" onClick={onCommands}><span>⌕</span> Command <kbd>⌘ K</kbd></button></div>
  </header>;
}

function Sidebar() {
  const view = useProfile(s => s.view);
  const setView = useProfile(s => s.setView);
  const index = useProfile(s => s.index);
  return <aside className="sidebar"><div className="side-label">WORKSPACE</div><nav aria-label="Views">{NAV.map(item => <button key={item.id} className={`nav-item ${view === item.id ? 'active' : ''}`} onClick={() => setView(item.id)}><span className="nav-icon">{item.icon}</span>{item.label}</button>)}</nav><div className="side-bottom"><div className="side-label">CAPTURE</div><div className="side-stat"><span>Spans</span><strong>{index?.profile.spans.length ?? '—'}</strong></div><div className="side-stat"><span>Chains</span><strong>{index?.profile.chains.length ?? '—'}</strong></div><div className="side-stat"><span>Roots</span><strong>{index?.profile.roots.length ?? '—'}</strong></div><div className="side-version">spanscope 0.1 · offline report</div></div></aside>;
}

function EmptyState({ onOpen, error, progress }: { onOpen: () => void; error: string; progress: number | null }) {
  return <div className="empty-stage"><div className="empty-grid" /><div className="empty-content"><div className="eyebrow"><span className="pulse-dot" /> PROFILE WORKSPACE</div><h1>See where your<br /><em>execution goes.</em></h1><p>Drop in a spanscope profile to explore call chains, root runs and the work beneath every span. Your data stays in this browser.</p><button className="primary-button" onClick={onOpen}>↥ &nbsp; Open profile <span>JSON or GZIP</span></button>{bootstrap.mode === 'picker' && <p className="hint">Large profile ready nearby: choose <strong>{bootstrap.suggested ?? 'the sidecar file'}</strong> to load it.</p>}{error && <div className="error-box" role="alert">{error}</div>}{progress !== null && <div className="progress"><div style={{ width: `${progress * 100}%` }} /></div>}</div><div className="empty-art" aria-hidden="true"><div className="art-line a" /><div className="art-line b" /><div className="art-line c" /><div className="art-line d" /><div className="art-line e" /><span className="art-label">root::request</span><span className="art-label second">db::query</span></div></div>;
}

function StatCard({ label, value, detail, accent }: { label: string; value: string; detail: string; accent?: boolean }) {
  return <div className={`stat-card ${accent ? 'accent' : ''}`}><div className="stat-label">{label}</div><div className="stat-value">{value}</div><div className="stat-detail">{detail}</div></div>;
}

function FilterBar({ index }: { index: ProfileIndex }) {
  const query = useProfile(s => s.query);
  const setQuery = useProfile(s => s.setQuery);
  const spanId = useProfile(s => s.spanId);
  const rootUid = useProfile(s => s.rootUid);
  const selectSpan = useProfile(s => s.selectSpan);
  const selectRoot = useProfile(s => s.selectRoot);
  const reset = useProfile(s => s.reset);
  return <div className="filterbar"><div className="search-wrap"><span>⌕</span><input aria-label="Search spans" placeholder="Filter by span name…" value={query} onChange={e => setQuery(e.target.value)} /></div><select aria-label="Filter by root" value={rootUid ?? ''} onChange={e => selectRoot(e.target.value || null)}><option value="">All roots</option>{index.profile.roots.map(root => <option key={root.uid} value={root.uid}>{root.uid.slice(0, 12)} · {formatNs(root.duration_ns)}</option>)}</select>{spanId !== null && <button className="filter-chip" onClick={() => selectSpan(null)}>Span: {index.spans.get(spanId)?.name ?? spanId} ×</button>}{(query || rootUid || spanId !== null) && <button className="reset-button" onClick={reset}>Clear filters</button>}</div>;
}

function OverviewChart({ index, chains }: { index: ProfileIndex; chains: ExactChain[] }) {
  const container = useRef<HTMLDivElement>(null);
  const selectChain = useProfile(s => s.selectChain);
  const setView = useProfile(s => s.setView);
  useEffect(() => {
    if (!container.current) return;
    const chart = echarts.init(container.current, undefined, { renderer: 'canvas' });
    const narrow = container.current.clientWidth < 500;
    const top = [...chains].sort((a,b) => asNumber(b.self_ns) - asNumber(a.self_ns)).slice(0, 8).reverse();
    const labels = top.map(chain => index.spans.get(asNumber(chain.path[chain.path.length - 1]))?.name.split('::').slice(-2).join('::') ?? `#${chain.id}`);
    chart.setOption({ animation: false, backgroundColor: 'transparent', grid: { left: narrow ? 96 : 125, right: 24, top: 8, bottom: 30 }, xAxis: { type: 'value', splitNumber: narrow ? 2 : 5, axisLabel: { hideOverlap: true, color: '#7d9195', formatter: (value: number) => formatNs(value) }, splitLine: { lineStyle: { color: '#27383b' } } }, yAxis: { type: 'category', data: labels, axisLabel: { color: '#acc1c2', width: 110, overflow: 'truncate' }, axisLine: { show: false }, axisTick: { show: false } }, tooltip: { trigger: 'item', formatter: (params: { dataIndex: number }) => `${labels[params.dataIndex]}<br/>Self: ${formatNs(top[params.dataIndex].self_ns)}` }, series: [{ type: 'bar', data: top.map(chain => asNumber(chain.self_ns)), barMaxWidth: 18, itemStyle: { color: '#65dec4', borderRadius: [0, 4, 4, 0] } }] });
    chart.on('click', (event: { dataIndex: number }) => { const chain = top[event.dataIndex]; if (chain) { selectChain(asNumber(chain.id), asNumber(chain.path[chain.path.length - 1])); setView('chains'); } });
    const observer = new ResizeObserver(() => chart.resize()); observer.observe(container.current);
    return () => { observer.disconnect(); chart.dispose(); };
  }, [chains, index, selectChain, setView]);
  return <div ref={container} className="overview-chart" role="img" aria-label="Top call chains by self time" />;
}

function Overview({ index, chains }: { index: ProfileIndex; chains: ExactChain[] }) {
  const capture = index.profile.meta.capture;
  const selectRoot = useProfile(s => s.selectRoot);
  const setView = useProfile(s => s.setView);
  return <div className="panel-stack"><div className="stats-grid"><StatCard label="Capture duration" value={formatNs(index.profile.meta.duration_ns)} detail={index.profile.meta.started_at} accent /><StatCard label="Observed calls" value={formatInteger(index.totalCalls)} detail={`${index.profile.chains.length} unique chains`} /><StatCard label="Active self time" value={formatNs(index.totalSelfNs)} detail="Across all chains" /><StatCard label="Root runs" value={formatInteger(index.profile.roots.length)} detail={`${index.profile.threads.length} participating threads`} /></div><div className="quick-paths"><button onClick={() => setView('routines')}><strong>Which routine costs most?</strong><span>Calls, self weight, callers and children →</span></button><button onClick={() => setView('runs')}><strong>What happened in one run?</strong><span>Per-run composition and metrics →</span></button><button onClick={() => setView('dependencies')}><strong>Who calls whom?</strong><span>Interactive graph and direct edges →</span></button></div><div className="overview-grid"><section className="panel chart-panel"><div className="panel-heading"><div><div className="eyebrow">HOT PATHS</div><h2>Where time accumulates</h2></div><span className="subtle">Exclusive active time</span></div>{chains.length ? <OverviewChart index={index} chains={chains} /> : <div className="no-results">No chains match the current filters.</div>}</section><section className="panel health-panel"><div className="panel-heading"><div><div className="eyebrow">CAPTURE QUALITY</div><h2>What was observed</h2></div></div><div className="health-row"><span>Snapshot</span><strong className={capture.snapshot_complete ? 'good' : 'warn'}>{capture.snapshot_complete ? 'Complete' : 'Pending threads'}</strong></div><div className="health-row"><span>Sampling rate</span><strong>{Math.round(asNumber(capture.sample_rate) * 100)}%</strong></div><div className="health-row"><span>Sampled out</span><strong>{formatInteger(capture.sampled_out_roots)}</strong></div><div className="health-row"><span>Dropped records</span><strong>{formatInteger(capture.dropped_records)}</strong></div><div className="health-row"><span>Evicted roots</span><strong>{formatInteger(capture.evicted_roots)}</strong></div><p className="panel-note">Wall times can overlap across threads. Root interval evidence is {index.profile.roots.some(root => root.execution.status === 'truncated') ? 'partially truncated' : 'partial'}; exact causal paths are unavailable.</p></section></div><section className="panel roots-panel"><div className="panel-heading"><div><div className="eyebrow">RUNS</div><h2>Recent roots</h2></div><span className="subtle">{index.profile.roots.length} retained</span></div><div className="root-list">{index.profile.roots.slice(-8).reverse().map(root => <button key={root.uid} className="root-row" onClick={() => { selectRoot(root.uid); setView('runs'); }}><span className="root-icon">◇</span><span className="root-name">{index.spans.get(asNumber(root.span))?.name ?? root.span}<small>{root.uid}</small></span><span className={`root-status ${root.completion}`}>{root.completion}</span><strong>{formatNs(root.duration_ns)}</strong><span className="root-arrow">↗</span></button>)}</div></section></div>;
}

interface FlameRect { x: number; y: number; width: number; height: number; spanId: number; key: string; }
function Flame({ index, chains }: { index: ProfileIndex; chains: ExactChain[] }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const layout = useRef<FlameRect[]>([]);
  const [hover, setHover] = useState<FlameRect | null>(null);
  const selectSpan = useProfile(s => s.selectSpan);
  const spanId = useProfile(s => s.spanId);
  const nodes = useMemo(() => buildFlame(chains), [chains]);
  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    const draw = () => {
      const bounds = element.getBoundingClientRect();
      const ratio = window.devicePixelRatio || 1;
      element.width = Math.max(1, Math.floor(bounds.width * ratio));
      element.height = Math.max(1, Math.floor(bounds.height * ratio));
      const ctx = element.getContext('2d'); if (!ctx) return;
      ctx.scale(ratio, ratio); ctx.clearRect(0,0,bounds.width,bounds.height);
      const children = new Map<string | null, typeof nodes>();
      for (const node of nodes) children.set(node.parent, [...(children.get(node.parent) ?? []), node]);
      const rects: FlameRect[] = [];
      const paint = (parent: string | null, left: number, width: number) => {
        const siblings = children.get(parent) ?? [];
        const total = siblings.reduce((sum, node) => sum + Number(node.weight), 0);
        let x = left;
        for (const node of siblings) {
          const w = total ? width * Number(node.weight) / total : 0;
          if (w < 1) { x += w; continue; }
          const y = node.depth * 36 + 14;
          const rect = { x, y, width: Math.max(0,w - 3), height: 31, spanId: node.spanId, key: node.key };
          rects.push(rect);
          const selected = spanId === node.spanId;
          const hue = 165 + (node.spanId * 29) % 38;
          ctx.fillStyle = selected ? '#dffef4' : `hsl(${hue} 49% ${30 + (node.depth % 3) * 6}%)`;
          ctx.fillRect(rect.x, rect.y, rect.width, rect.height);
          if (rect.width > 50) { ctx.fillStyle = selected ? '#0b2322' : '#e8f7ef'; ctx.font = '12px Inter, system-ui, sans-serif'; ctx.fillText(index.spans.get(node.spanId)?.name.split('::').pop() ?? String(node.spanId), x + 9, y + 20, Math.max(0, rect.width - 17)); }
          paint(node.key, x, w);
          x += w;
        }
      };
      paint(null, 0, bounds.width);
      layout.current = rects;
    };
    draw(); const observer = new ResizeObserver(draw); observer.observe(element); return () => observer.disconnect();
  }, [nodes, index, spanId]);
  return <div className="panel flame-panel"><div className="panel-heading"><div><div className="eyebrow">STRUCTURAL VIEW</div><h2>Flame graph</h2></div><span className="subtle">Widths use aggregate active self time · click a span to filter</span></div><div className="flame-scroll"><canvas ref={canvas} style={{ height: `${Math.max(230, (Math.max(0, ...nodes.map(node => node.depth)) + 1) * 36 + 30)}px` }} onMouseMove={event => { const b = event.currentTarget.getBoundingClientRect(); const x = event.clientX - b.left, y = event.clientY - b.top; setHover(layout.current.slice().reverse().find((rect: FlameRect) => x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height) ?? null); }} onMouseLeave={() => setHover(null)} onClick={() => hover && selectSpan(hover.spanId)} role="img" aria-label="Interactive structural flame graph" />{hover && <div className="flame-tooltip">{index.spans.get(hover.spanId)?.name}</div>}</div><p className="panel-note">This is an aggregate call hierarchy, not an execution timeline. Concurrent wall durations are not additive.</p></div>;
}

const column = createColumnHelper<ExactChain>();
function ChainsTable({ index, chains }: { index: ProfileIndex; chains: ExactChain[] }) {
  const [sorting, setSorting] = useState<SortingState>([{ id: 'self_ns', desc: true }]);
  const selected = useProfile(s => s.chainId);
  const selectChain = useProfile(s => s.selectChain);
  const container = useRef<HTMLDivElement>(null);
  const columns = useMemo(() => [
    column.accessor(row => asNumber(row.id), { id: 'id', header: 'ID', cell: value => `#${value.getValue()}` }),
    column.accessor(row => row.path.map(id => index.spans.get(asNumber(id))?.name ?? id).join(' › '), { id: 'path', header: 'Call chain', cell: value => <span className="path-cell" title={value.getValue()}>{value.getValue()}</span> }),
    column.accessor(row => asNumber(row.count), { id: 'count', header: 'Calls', cell: value => formatInteger(value.row.original.count) }),
    column.accessor(row => asNumber(row.total_ns), { id: 'total_ns', header: 'Total wall', cell: value => formatNs(value.row.original.total_ns) }),
    column.accessor(row => asNumber(row.self_ns), { id: 'self_ns', header: 'Self active', cell: value => formatNs(value.row.original.self_ns) }),
    column.accessor(row => asNumber(row.p99_ns), { id: 'p99_ns', header: 'P99', cell: value => formatNs(value.row.original.p99_ns) }),
  ], [index]);
  const table = useReactTable({ data: chains, columns, state: { sorting }, onSortingChange: setSorting, getCoreRowModel: getCoreRowModel(), getSortedRowModel: getSortedRowModel() });
  const rows = table.getRowModel().rows;
  const virtualizer = useVirtualizer({ count: rows.length, getScrollElement: () => container.current, estimateSize: () => 44, overscan: 10 });
  return <div className="panel table-panel"><div className="panel-heading"><div><div className="eyebrow">OBSERVATIONS</div><h2>Call chains</h2></div><span className="subtle">{chains.length} matching chains</span></div><div className="table-header">{table.getHeaderGroups()[0].headers.map(header => <button key={header.id} onClick={header.column.getToggleSortingHandler()} className={`table-cell col-${header.id}`}>{flexRender(header.column.columnDef.header, header.getContext())}{header.column.getIsSorted() === 'asc' ? ' ↑' : header.column.getIsSorted() === 'desc' ? ' ↓' : ''}</button>)}</div><div ref={container} className="table-scroll"><div style={{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }}>{virtualizer.getVirtualItems().map(item => { const row = rows[item.index]; return <button key={row.id} className={`table-row ${selected === asNumber(row.original.id) ? 'selected' : ''}`} style={{ transform: `translateY(${item.start}px)` }} onClick={() => selectChain(asNumber(row.original.id), asNumber(row.original.path[row.original.path.length - 1]))}>{row.getVisibleCells().map(cell => <span key={cell.id} className={`table-cell col-${cell.column.id}`}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</span>)}</button>; })}</div></div>{chains.length === 0 && <div className="no-results">No chains match these filters.</div>}</div>;
}

function RawExplorer({ index }: { index: ProfileIndex }) {
  const chainId = useProfile(s => s.chainId);
  const rootUid = useProfile(s => s.rootUid);
  const spanId = useProfile(s => s.spanId);
  const selected: unknown = chainId !== null ? index.chains.get(chainId) : rootUid ? index.roots.get(rootUid) : spanId !== null ? index.spans.get(spanId) : { meta: index.profile.meta, analysis: index.profile.analysis };
  return <div className="panel raw-panel"><div className="panel-heading"><div><div className="eyebrow">SOURCE DATA</div><h2>Raw explorer</h2></div><span className="subtle">Exact large integers shown as decimal strings</span></div><div className="raw-selector">{chainId !== null ? `Chain #${chainId}` : rootUid ? `Root ${rootUid}` : spanId !== null ? `Span #${spanId}` : 'Metadata & analysis'}</div><pre>{exactJson(selected)}</pre></div>;
}

function CommandPalette({ open, setOpen, onOpen }: { open: boolean; setOpen: (value: boolean) => void; onOpen: () => void }) {
  const [query, setQuery] = useState('');
  const setView = useProfile(s => s.setView);
  const reset = useProfile(s => s.reset);
  const commands = [ ...NAV.map(item => ({ label: `Go to ${item.label}`, hint: item.icon, run: () => setView(item.id) })), { label: 'Open a profile', hint: '↥', run: onOpen }, { label: 'Clear filters', hint: '×', run: reset } ].filter(item => item.label.toLowerCase().includes(query.toLowerCase()));
  return <Dialog.Root open={open} onOpenChange={setOpen}><Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog-content"><Dialog.Title>Command palette</Dialog.Title><input autoFocus placeholder="What would you like to do?" value={query} onChange={event => setQuery(event.target.value)} /><div className="command-list">{commands.map(item => <button key={item.label} onClick={() => { item.run(); setOpen(false); setQuery(''); }}><span>{item.hint}</span>{item.label}<kbd>↵</kbd></button>)}</div><p>ESC to close · Ctrl / ⌘ + K to open</p></Dialog.Content></Dialog.Portal></Dialog.Root>;
}

export default function App() {
  const fileInput = useRef<HTMLInputElement>(null);
  const [commands, setCommands] = useState(false);
  const { load, error, progress } = useLoader();
  const index = useProfile(s => s.index);
  const view = useProfile(s => s.view);
  const query = useProfile(s => s.query);
  const spanId = useProfile(s => s.spanId);
  const rootUid = useProfile(s => s.rootUid);
  const chains = useMemo(() => index ? filterChains(index, { query, spanId, rootUid }) : [], [index, query, spanId, rootUid]);
  const open = useCallback(() => fileInput.current?.click(), []);
  useEffect(() => { const keydown = (event: KeyboardEvent) => { if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') { event.preventDefault(); setCommands(value => !value); } }; window.addEventListener('keydown', keydown); return () => window.removeEventListener('keydown', keydown); }, []);
  useEffect(() => { const drag = (event: DragEvent) => event.preventDefault(); const drop = (event: DragEvent) => { event.preventDefault(); const file = event.dataTransfer?.files[0]; if (file) void load(file); }; window.addEventListener('dragover', drag); window.addEventListener('drop', drop); return () => { window.removeEventListener('dragover', drag); window.removeEventListener('drop', drop); }; }, [load]);
  return <div className="app-shell"><TopBar onOpen={open} onCommands={() => setCommands(true)} /><div className="shell-body"><Sidebar /><main className="main-content">{!index ? <EmptyState onOpen={open} error={error} progress={progress} /> : <><div className="page-header"><div><div className="eyebrow">EXECUTION PROFILE / {view.toUpperCase()}</div><h1>{NAV.find(item => item.id === view)?.label}</h1><p>{index.profile.meta.program} · {index.profile.meta.started_at}</p></div><button className="outline-button" onClick={open}>↥ &nbsp; New profile</button></div><FilterBar index={index} />{error && <div className="error-box" role="alert">{error}</div>}{progress !== null && <div className="progress"><div style={{ width: `${progress * 100}%` }} /></div>}{view === 'overview' && <Overview index={index} chains={chains} />}{view === 'routines' && <RoutinesView index={index} rootUid={rootUid} query={query} />}{view === 'runs' && <RunsView index={index} rootUid={rootUid} />}{view === 'dependencies' && <DependenciesView index={index} rootUid={rootUid} />}{view === 'flame' && <Flame index={index} chains={chains} />}{view === 'chains' && <ChainsTable index={index} chains={chains} />}{view === 'raw' && <RawExplorer index={index} />}</>}</main></div><input ref={fileInput} hidden type="file" accept=".json,.gz,.json.gz,application/json,application/gzip" onChange={event => { const file = event.target.files?.[0]; if (file) void load(file); event.currentTarget.value = ''; }} /><CommandPalette open={commands} setOpen={setCommands} onOpen={open} /></div>;
}
