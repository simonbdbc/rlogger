/** Presentation only: the caller keeps the original RLOG/1 text unchanged. */
export interface VisualLine {
  text: string;
  kind: "raw" | "header" | "json" | "separator";
}

export interface DisplayLines {
  lines: VisualLine[];
  /** Start of each physical line, followed by the end of the visual window. */
  physicalStarts: number[];
}

const TIMESTAMP =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}(?:Z|[+-]\d{2}:\d{2})$/;
const PREFIX =
  /^RLOG\/1 (\S+) (TRACE|DEBUG|INFO|WARN|ERROR) \[LATENCY — (\d+)ms\] \[/;
const KEY = /[A-Za-z0-9_]/;

/** RLOG/1 escapes are deliberately narrower than JavaScript string escapes. */
function decodeEscapes(input: string): string | null {
  let result = "";
  for (let i = 0; i < input.length; i++) {
    const char = input[i]!;
    if (char !== "\\") {
      result += char;
      continue;
    }
    const next = input[++i];
    if (next === undefined) return null;
    if (next === "n") result += "\n";
    else if (next === "r") result += "\r";
    else if (next === "t") result += "\t";
    else if (next === "\\") result += "\\";
    else if (next === '"') result += '"';
    else if (next === "u") {
      const match = /^\{([0-9a-fA-F]{1,6})\}/.exec(input.slice(i + 1));
      if (!match) return null;
      const codePoint = Number.parseInt(match[1]!, 16);
      if (codePoint > 0x10ffff || (codePoint >= 0xd800 && codePoint <= 0xdfff))
        return null;
      result += String.fromCodePoint(codePoint);
      i += match[0].length;
    } else return null;
  }
  return result;
}

function metadataFields(input: string): Map<string, string> | null {
  if (!input) return null;
  const fields = new Map<string, string>();
  let i = 0;
  while (i < input.length) {
    const start = i;
    while (i < input.length && KEY.test(input[i]!)) i++;
    if (i === start || input[i] !== "=") return null;
    const key = input.slice(start, i++);
    if (fields.has(key)) return null;
    let value: string;
    if (input[i] === '"') {
      const valueStart = ++i;
      let escaped = false;
      while (i < input.length) {
        const char = input[i]!;
        if (escaped) escaped = false;
        else if (char === "\\") escaped = true;
        else if (char === '"') break;
        i++;
      }
      if (i === input.length) return null;
      const decoded = decodeEscapes(input.slice(valueStart, i++));
      if (decoded === null) return null;
      value = decoded;
    } else {
      const valueStart = i;
      while (i < input.length && input[i] !== " ") i++;
      value = input.slice(valueStart, i);
      if (
        !value ||
        value.includes('"') ||
        value.includes("[") ||
        value.includes("]") ||
        value.includes("\\")
      )
        return null;
    }
    fields.set(key, value);
    if (i < input.length) {
      if (input[i++] !== " " || i === input.length || input[i] === " ")
        return null;
    }
  }
  if (fields.has("seq") && !/^\d+$/.test(fields.get("seq")!)) return null;
  return fields;
}

function readableEvent(line: string): VisualLine[] | null {
  const prefix = PREFIX.exec(line);
  if (!prefix) return null;
  const [, timestamp, level, latency] = prefix;
  if (!TIMESTAMP.test(timestamp!) || Number.isNaN(Date.parse(timestamp!)))
    return null;
  const metadataStart = prefix[0].length;
  let inQuotes = false;
  let escaped = false;
  let end = metadataStart;
  for (; end < line.length; end++) {
    const char = line[end]!;
    if (escaped) escaped = false;
    else if (inQuotes && char === "\\") escaped = true;
    else if (char === '"') inQuotes = !inQuotes;
    else if (char === "]" && !inQuotes) break;
  }
  if (inQuotes || line[end] !== "]" || line[end + 1] !== " ") return null;
  const fields = metadataFields(line.slice(metadataStart, end));
  if (
    !fields ||
    !/^[a-z0-9_-]{1,64}$/.test(fields.get("instance") ?? "") ||
    !fields.has("source")
  )
    return null;
  const message = decodeEscapes(line.slice(end + 2));
  if (message === null) return null;
  let pretty: string;
  try {
    pretty = JSON.stringify(JSON.parse(message), null, 2);
  } catch {
    return null;
  }
  if (typeof pretty !== "string") return null;
  const seq = fields.get("seq");
  const action = (fields.get("action") ?? "Événement RLOGGER")
    .replaceAll("\n", "\\n")
    .replaceAll("\r", "\\r")
    .replaceAll("\t", "\\t")
    .replaceAll("\u2028", String.raw`\u{2028}`)
    .replaceAll("\u2029", String.raw`\u{2029}`);
  const header = [
    timestamp!,
    level!,
    action,
    ...(seq === undefined ? [] : [`#${seq}`]),
    `${latency} ms`,
  ].join("  ·  ");
  return [
    { text: header, kind: "header" },
    ...pretty
      .replaceAll("\u2028", String.raw`\u2028`)
      .replaceAll("\u2029", String.raw`\u2029`)
      .split("\n")
      .map((text): VisualLine => ({ text, kind: "json" })),
    { text: "", kind: "separator" },
  ];
}

export function displayLines(
  text: string,
  readable: boolean,
  partialStart = false,
): DisplayLines {
  const lines: VisualLine[] = [];
  const physicalStarts: number[] = [];
  if (!text) return { lines, physicalStarts: [0] };
  const physical = text.split("\n");
  for (let i = 0; i < physical.length; i++) {
    physicalStarts.push(lines.length);
    const original = physical[i]!;
    const transformed =
      readable && i < physical.length - 1 && !(partialStart && i === 0)
        ? readableEvent(original)
        : null;
    if (transformed) lines.push(...transformed);
    else lines.push({ text: original, kind: "raw" });
  }
  physicalStarts.push(lines.length);
  return { lines, physicalStarts };
}
