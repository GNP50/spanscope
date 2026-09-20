import Ajv from 'ajv';
import schema from '../../schema/profile-v1.schema.json';
import { parseLosslessJson, validateAndIndex } from './core';

const validate = new Ajv({ strict: false, allErrors: false }).compile(schema);

self.onmessage = (event: MessageEvent<{ id: number; text: string }>) => {
  const { id, text } = event.data;
  try {
    self.postMessage({ id, type: 'progress', value: 0.1 });
    const ordinary = JSON.parse(text);
    if (!validate(ordinary)) {
      const error = validate.errors?.[0];
      throw new Error(`Schema v1 validation failed at ${error?.instancePath || '/'}: ${error?.message ?? 'invalid profile'}`);
    }
    const profile = parseLosslessJson(text);
    self.postMessage({ id, type: 'progress', value: 0.7 });
    const index = validateAndIndex(profile);
    self.postMessage({ id, type: 'ready', index });
  } catch (error) {
    self.postMessage({ id, type: 'error', message: error instanceof Error ? error.message : String(error) });
  }
};
