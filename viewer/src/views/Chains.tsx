import { useMemo, useRef, useState } from 'react';
import { createColumnHelper, flexRender, getCoreRowModel, getSortedRowModel, useReactTable, type SortingState } from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { asNumber, formatInteger, formatNs, spanName, type ExactChain, type ProfileIndex } from '../model';
import { useProfile } from '../store';
import { EntityLink, Panel, ViewLinks } from '../ui/components';

const column = createColumnHelper<ExactChain>();

function SelectedChain({ index, chainId }: { index: ProfileIndex; chainId: number }) {
  const chain = index.chains.get(chainId);
  if (!chain) return null;
  return (
    <section className="panel selection-strip">
      <div>
        <div className="eyebrow">SELECTED CHAIN #{chainId}</div>
        <div className="chain-path">
          {chain.path.map((id, position) => (
            <span key={position}>
              {position > 0 && <b>→</b>}
              <EntityLink entity={{ kind: 'span', id: asNumber(id) }} view="stay" />
            </span>
          ))}
        </div>
      </div>
      <div className="strip-stats">
        <span>
          <small>Calls</small>
          <strong>{formatInteger(chain.count)}</strong>
        </span>
        <span>
          <small>Self active</small>
          <strong>{formatNs(chain.self_ns)}</strong>
        </span>
        <span>
          <small>P99</small>
          <strong>{formatNs(chain.p99_ns)}</strong>
        </span>
      </div>
      <ViewLinks
        entity={{ kind: 'chain', id: chainId }}
        views={[
          { view: 'flame', label: 'Flame' },
          { view: 'routines', label: 'Routine' },
          { view: 'raw', label: 'Raw' },
        ]}
      />
    </section>
  );
}

export function Chains({ index, chains }: { index: ProfileIndex; chains: ExactChain[] }) {
  const [sorting, setSorting] = useState<SortingState>([{ id: 'self_ns', desc: true }]);
  const selected = useProfile(state => state.chainId);
  const navigate = useProfile(state => state.navigate);
  const container = useRef<HTMLDivElement>(null);
  const columns = useMemo(
    () => [
      column.accessor(row => asNumber(row.id), { id: 'id', header: 'ID', cell: value => `#${value.getValue()}` }),
      column.accessor(row => row.path.map(id => spanName(index, asNumber(id))).join(' › '), {
        id: 'path',
        header: 'Call chain',
        cell: value => (
          <span className="path-cell" title={value.getValue()}>
            {value.getValue()}
          </span>
        ),
      }),
      column.accessor(row => asNumber(row.count), { id: 'count', header: 'Calls', cell: value => formatInteger(value.row.original.count) }),
      column.accessor(row => asNumber(row.total_ns), { id: 'total_ns', header: 'Total wall', cell: value => formatNs(value.row.original.total_ns) }),
      column.accessor(row => asNumber(row.self_ns), { id: 'self_ns', header: 'Self active', cell: value => formatNs(value.row.original.self_ns) }),
      column.accessor(row => asNumber(row.p99_ns), { id: 'p99_ns', header: 'P99', cell: value => formatNs(value.row.original.p99_ns) }),
    ],
    [index],
  );
  const table = useReactTable({
    data: chains,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  });
  const rows = table.getRowModel().rows;
  const virtualizer = useVirtualizer({ count: rows.length, getScrollElement: () => container.current, estimateSize: () => 44, overscan: 10 });
  return (
    <div className="panel-stack">
      {selected !== null && <SelectedChain index={index} chainId={selected} />}
      <Panel eyebrow="OBSERVATIONS" title="Call chains" aside={`${chains.length} matching chains`} className="table-panel">
        <div className="table-header">
          {table.getHeaderGroups()[0].headers.map(header => (
            <button key={header.id} onClick={header.column.getToggleSortingHandler()} className={`table-cell col-${header.id}`}>
              {flexRender(header.column.columnDef.header, header.getContext())}
              {header.column.getIsSorted() === 'asc' ? ' ↑' : header.column.getIsSorted() === 'desc' ? ' ↓' : ''}
            </button>
          ))}
        </div>
        <div ref={container} className="table-scroll">
          <div style={{ height: `${virtualizer.getTotalSize()}px`, position: 'relative' }}>
            {virtualizer.getVirtualItems().map(item => {
              const row = rows[item.index];
              const id = asNumber(row.original.id);
              return (
                <button
                  key={row.id}
                  className={`table-row ${selected === id ? 'selected' : ''}`}
                  style={{ transform: `translateY(${item.start}px)` }}
                  onClick={() => navigate({ kind: 'chain', id }, 'stay')}
                >
                  {row.getVisibleCells().map(cell => (
                    <span key={cell.id} className={`table-cell col-${cell.column.id}`}>
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </span>
                  ))}
                </button>
              );
            })}
          </div>
        </div>
        {chains.length === 0 && <div className="no-results">No chains match these filters.</div>}
      </Panel>
    </div>
  );
}
