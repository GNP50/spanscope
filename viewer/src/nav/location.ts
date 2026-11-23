import { asNumber, leafSpan, type ProfileIndex } from '../model';
import { isView, type Entity, type View } from './views';

/** Pure URL and selection logic, testable without a browser. */
export interface Selection {
  spanId: number | null;
  chainId: number | null;
  rootUid: string | null;
  typeId: number | null;
  instanceId: number | null;
}

export interface Location extends Selection {
  view: View;
  query: string;
}

export const EMPTY: Selection = { spanId: null, chainId: null, rootUid: null, typeId: null, instanceId: null };

const integer = (value: string | null): number | null => (value !== null && /^\d+$/.test(value) ? Number(value) : null);

export function locationFromHash(hash: string): Location {
  const params = new URLSearchParams(hash.replace(/^#/, ''));
  const view = params.get('view');
  return {
    view: isView(view) ? view : 'overview',
    query: params.get('q') ?? '',
    spanId: integer(params.get('span')),
    chainId: integer(params.get('chain')),
    rootUid: params.get('root'),
    typeId: integer(params.get('type')),
    instanceId: integer(params.get('instance')),
  };
}

export function hashFromLocation(location: Location): string {
  const params = new URLSearchParams();
  params.set('view', location.view);
  if (location.spanId !== null) params.set('span', String(location.spanId));
  if (location.chainId !== null) params.set('chain', String(location.chainId));
  if (location.rootUid) params.set('root', location.rootUid);
  if (location.typeId !== null) params.set('type', String(location.typeId));
  if (location.instanceId !== null) params.set('instance', String(location.instanceId));
  if (location.query) params.set('q', location.query);
  return `#${params}`;
}

/** Selection change an entity implies; chains also select their leaf span, instances their type. */
export function selectionFor(entity: Entity, index: ProfileIndex | null): Partial<Selection> {
  switch (entity.kind) {
    case 'span':
      return { spanId: entity.id, chainId: null };
    case 'chain': {
      const chain = index?.chains.get(entity.id);
      return { chainId: entity.id, spanId: chain ? leafSpan(chain) : null };
    }
    case 'root':
      return { rootUid: entity.uid, chainId: null };
    case 'type':
      return { typeId: entity.id, instanceId: null };
    case 'instance': {
      const instance = index?.objects?.instances.get(entity.id);
      return { instanceId: entity.id, typeId: instance ? asNumber(instance.type_id) : null };
    }
  }
}
