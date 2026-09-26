import { test } from "node:test";
import assert from "node:assert/strict";
import { displayLines } from "../src/log-viewer/rlog-display.ts";

const stamp = "2026-09-25T13:09:20.391Z";
const event = (metadata: string, message: string) =>
  `RLOG/1 ${stamp} INFO [LATENCY — 15ms] [${metadata}] ${message}`;
const rendered = (text: string) =>
  displayLines(text, true)
    .lines.map((line) => line.text)
    .join("\n");

test("RLOG/1 JSON gets an ordered header, two-space indentation, and a blank separator", () => {
  const raw =
    event(
      'seq=1 instance=calendar source="calendar:runtime.rs:145" action="calendar.stage1.admission_started"',
      String.raw`{\"attempt\":0,\"force\":true,\"local_day\":\"2026-09-25\",\"trigger\":\"manual_admin\"}`,
    ) + "\n";
  assert.equal(
    rendered(raw),
    `${stamp}  ·  INFO  ·  calendar.stage1.admission_started  ·  #1  ·  15 ms\n` +
      '{\n  "attempt": 0,\n  "force": true,\n  "local_day": "2026-09-25",\n  "trigger": "manual_admin"\n}\n\n',
  );
  assert.deepEqual(
    displayLines(raw, true).lines.map((line) => line.kind),
    [
      "header",
      "json",
      "json",
      "json",
      "json",
      "json",
      "json",
      "separator",
      "raw",
    ],
  );
});

test("missing action and seq use the default label without inventing a sequence", () => {
  const raw = event('instance=calendar source="calendar:main.rs:1"', "[1,2]");
  assert.equal(
    rendered(raw + "\n").split("\n")[0],
    `${stamp}  ·  INFO  ·  Événement RLOGGER  ·  15 ms`,
  );
});

test("a closing bracket inside quoted metadata does not terminate it", () => {
  const raw = event(
    'seq=9 instance=calendar source="module:foo]bar" action="day]closed"',
    String.raw`{\"ok\":true}`,
  );
  assert.match(rendered(raw + "\n"), /day]closed  ·  #9/);
});

test("only RLOG/1 escapes are decoded, including Unicode scalar values", () => {
  const original = JSON.stringify({
    newline: "a\n b",
    tab: "\t",
    slash: "\\",
    quote: '"',
    emoji: "🦀",
    unicode: "\u2028",
  });
  const escaped = original
    .replaceAll("\\", "\\\\")
    .replaceAll('"', '\\"')
    .replaceAll("\u2028", String.raw`\u{2028}`);
  const raw = event(
    'seq=3 instance=calendar source="calendar:main.rs:1" action="calendar"',
    escaped,
  );
  const text = rendered(raw + "\n");
  assert.match(text, /"newline": "a\\n b"/);
  assert.match(text, /"tab": "\\t"/);
  assert.match(text, /"slash": "\\\\"/);
  assert.match(text, /"quote": "\\\""/);
  assert.match(text, /"emoji": "🦀"/);
  assert.ok(text.includes(String.raw`\u2028`));
});

test("multipart fields remain separate physical events", () => {
  const first = event(
    'seq=4 instance=calendar source="calendar:main.rs:1" part=1 last_part=false action="upload"',
    String.raw`{\"part\":1}`,
  );
  const second = event(
    'seq=5 instance=calendar source="calendar:main.rs:1" part=2 last_part=true action="upload"',
    String.raw`{\"part\":2}`,
  );
  const display = displayLines(`${first}\n${second}\n`, true);
  assert.equal(
    display.lines.filter((line) => line.kind === "header").length,
    2,
  );
  assert.equal(display.physicalStarts[1], 5);
});

test("ordinary, malformed, unknown escapes, incomplete JSON, and partial lines stay raw", () => {
  const metadata =
    'seq=1 instance=calendar source="calendar:main.rs:1" action="x"';
  const candidates = [
    "ordinary text",
    event(metadata, "hello"),
    event(metadata, String.raw`{\"a\":`),
    event(metadata, String.raw`{\x}`),
    event(
      'seq=oops instance=calendar source="calendar:main.rs:1" action="x"',
      String.raw`{\"a\":1}`,
    ),
    event(
      'seq=1 instance=calendar source="calendar:main.rs:1" action="unterminated',
      String.raw`{\"a\":1}`,
    ),
    event(metadata, String.raw`{\u{110000}}`),
    event('seq=1 action="x"', String.raw`{\"a\":1}`),
    event(metadata, String.raw`{\"a\":1}`),
  ];
  for (const raw of candidates.slice(0, -1)) {
    assert.equal(rendered(raw + "\n"), raw + "\n", raw);
  }
  assert.equal(
    rendered(candidates.at(-1)!),
    candidates.at(-1),
    "unterminated final physical line",
  );
  const valid = event(metadata, String.raw`{\"a\":1}`);
  assert.equal(
    displayLines(valid + "\n", true, true).lines[0]?.text,
    valid,
    "partial first line",
  );
});

test("raw mode reproduces the decoded input text exactly", () => {
  const raw =
    event('seq=1 action="x"', String.raw`{\"a\":1}`) + "\r\nplain 🦀\npartial";
  assert.equal(
    displayLines(raw, false)
      .lines.map((line) => line.text)
      .join("\n"),
    raw,
  );
});

test("physical-to-visual starts convert history prepends with unequal JSON heights", () => {
  const first = event(
    'seq=1 instance=calendar source="calendar:main.rs:1" action="a"',
    String.raw`{\"a\":1}`,
  );
  const second = event(
    'seq=2 instance=calendar source="calendar:main.rs:1" action="b"',
    String.raw`{\"x\":[1,2]}`,
  );
  const old = "older visible line\n";
  const display = displayLines(`${first}\n${second}\n${old}`, true);
  assert.equal(display.physicalStarts[0], 0);
  assert.equal(display.physicalStarts[1], 5);
  assert.equal(display.physicalStarts[2], 13);
  assert.equal(display.lines[13]?.text, "older visible line");
  assert.equal(display.physicalStarts.at(-1), display.lines.length);
});
