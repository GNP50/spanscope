import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { resolve, dirname } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const htmlPath = resolve(root, 'dist/index.html');
const html = readFileSync(htmlPath, 'utf8');
const script = html.match(/<script type="module"[^>]* src="([^"]+)"[^>]*><\/script>/);
const style = html.match(/<link rel="stylesheet"[^>]* href="([^"]+)"[^>]*>/);
if (!script || !style) throw new Error('Vite output changed: expected one JS and one CSS asset');
const asset = (reference) => resolve(root, 'dist', reference.replace(/^\.\//, ''));
const js = readFileSync(asset(script[1]), 'utf8').replace(/<\/script/gi, '<\\/script');
const css = readFileSync(asset(style[1]), 'utf8').replace(/<\/style/gi, '<\\/style');
// The report is a single file people copy around on its own, so the third-party
// attributions have to travel inside it, not only beside it in the repository.
const notices = readFileSync(resolve(root, '../THIRD-PARTY-NOTICES.md'), 'utf8').replace(/--+>/g, '-->');
const credit = `<!--\nThis report embeds third-party libraries. Their licenses follow.\nspanscope itself is MIT OR Apache-2.0.\n\n${notices}\n-->\n`;
const bundled = credit + html.replace(script[0], () => `<script type="module">${js}</script>`).replace(style[0], () => `<style>${css}</style>`).replace(/^ +$/gm, '');
if (/\b(?:src|href)="(?:\.?\/)?assets\//.test(bundled)) throw new Error('external asset reference remains');
if (!bundled.includes('__SPANSCOPE_BOOTSTRAP__')) throw new Error('bootstrap slot disappeared');
const outputs = [
  resolve(root, '../cargo-spanscope/assets/viewer.html'),
  resolve(root, '../spanscope/assets/viewer.html'),
];
if (process.argv.includes('--check')) {
  for (const path of outputs) {
    if (readFileSync(path, 'utf8') !== bundled) throw new Error(`${path} is stale: npm run build`);
  }
} else {
  for (const path of outputs) { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, bundled); }
  console.log(`single HTML asset: ${Buffer.byteLength(bundled)} bytes`);
}
