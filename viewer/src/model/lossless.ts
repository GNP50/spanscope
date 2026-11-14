const MAX_SAFE = BigInt(Number.MAX_SAFE_INTEGER);

/** Keep large JSON integer tokens exact before JSON.parse can round them. */
export function parseLosslessJson(text: string): unknown {
  let marker = '__SPANSCOPE_EXACT_INTEGER__';
  while (text.includes(marker)) marker += '_';
  let output = '';
  let quoted = false;
  let escaped = false;
  for (let i = 0; i < text.length;) {
    const character = text[i];
    if (quoted) {
      output += character;
      if (escaped) escaped = false;
      else if (character === '\\') escaped = true;
      else if (character === '"') quoted = false;
      i += 1;
      continue;
    }
    if (character === '"') {
      quoted = true;
      output += character;
      i += 1;
      continue;
    }
    if (character === '-' || (character >= '0' && character <= '9')) {
      const match = text.slice(i).match(/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/);
      if (!match) throw new Error(`Invalid number at ${i}`);
      const token = match[0];
      if (!/[.eE]/.test(token) && (token.length > 15 || (token.length === 16 && token[0] === '-'))) {
        const integer = BigInt(token);
        output += integer > MAX_SAFE || integer < -MAX_SAFE ? `"${marker}${token}"` : token;
      } else output += token;
      i += token.length;
      continue;
    }
    output += character;
    i += 1;
  }
  return JSON.parse(output, (_key, value: unknown) => {
    if (typeof value === 'string' && value.startsWith(marker)) {
      const digits = value.slice(marker.length);
      if (/^-?\d+$/.test(digits)) return BigInt(digits);
    }
    return value;
  });
}
