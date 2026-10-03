import Ajv from 'ajv';
import schema from '../../schema/profile-v1.schema.json';
import { parseLosslessJson, validateAndIndex } from './core';

const validate = new Ajv({ strict: false, allErrors: false }).compile(schema);

async function readProfileFile(file: File): Promise<string> {
  const magic = new Uint8Array(await file.slice(0, 2).arrayBuffer());
  if (magic[0] === 0x1f && magic[1] === 0x8b) {
    if (typeof DecompressionStream === 'undefined') throw new Error('This browser cannot decompress gzip files.');
    return new Response(file.stream().pipeThrough(new DecompressionStream('gzip'))).text();
  }
  return file.text();
}

self.onmessage = async (event: MessageEvent<{ id: number; text?: string; file?: File }>) => {
  const { id } = event.data;
  try {
    const text = event.data.text ?? (event.data.file ? await readProfileFile(event.data.file) : null);
    if (text === null) throw new Error('No profile payload was provided.');
    self.postMessage({ id, type: 'progress', value: 0.1 });
    let ordinary: unknown = JSON.parse(text);
    if (!validate(ordinary)) {
      const error = validate.errors?.[0];
      throw new Error(`Schema v1 validation failed at ${error?.instancePath || '/'}: ${error?.message ?? 'invalid profile'}`);
    }
    ordinary = null;
    const profile = parseLosslessJson(text);
    self.postMessage({ id, type: 'progress', value: 0.7 });
    const index = validateAndIndex(profile);
    self.postMessage({ id, type: 'ready', index });
  } catch (error) {
    self.postMessage({ id, type: 'error', message: error instanceof Error ? error.message : String(error) });
  }
};
