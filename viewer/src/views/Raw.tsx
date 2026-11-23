import { exactJson, type ProfileIndex } from '../model';
import { useProfile } from '../store';
import { Panel } from '../ui/components';

/** Exact source record of the most specific selection. */
export function Raw({ index }: { index: ProfileIndex }) {
  const { chainId, rootUid, spanId, instanceId, typeId } = useProfile(state => state);
  const objects = index.objects;
  let title = 'Metadata & analysis';
  let selected: unknown = { meta: index.profile.meta, analysis: index.profile.analysis };
  if (instanceId !== null && objects?.instances.has(instanceId)) {
    title = `Instance #${instanceId}`;
    selected = objects.instances.get(instanceId);
  } else if (typeId !== null && objects?.types.has(typeId)) {
    title = `Type #${typeId}`;
    selected = {
      type: objects.types.get(typeId),
      fields: objects.fieldsByType.get(typeId) ?? [],
      sites: objects.sitesByType.get(typeId) ?? [],
    };
  } else if (chainId !== null) {
    title = `Chain #${chainId}`;
    selected = index.chains.get(chainId);
  } else if (rootUid) {
    title = `Root ${rootUid}`;
    selected = index.roots.get(rootUid);
  } else if (spanId !== null) {
    title = `Span #${spanId}`;
    selected = index.spans.get(spanId);
  }
  return (
    <Panel eyebrow="SOURCE DATA" title="Raw explorer" aside="Exact large integers shown as decimal strings" className="raw-panel">
      <div className="raw-selector">{title}</div>
      <pre>{exactJson(selected)}</pre>
    </Panel>
  );
}
