import { test } from "node:test";
import assert from "node:assert/strict";
import { ByteWindow } from "../src/log-viewer/window.ts";
import type { Chunk } from "../src/log-viewer/protocol.ts";
function chunk(data: Buffer, start: number): Chunk {
  return {
    generation: "one",
    start: String(start),
    end: String(start + data.length),
    size: String(start + data.length),
    data: data.toString("base64"),
    partialStart: start > 0,
  };
}
test("UTF-8 split at every byte, CRLF and incomplete final line preserve exact text", () => {
  const data = Buffer.from("début 🦀\r\n日本語\nfin");
  for (let split = 1; split < data.length; split++) {
    const w = new ByteWindow();
    w.replace(chunk(data.subarray(0, split), 0));
    w.append(chunk(data.subarray(split), split));
    assert.equal(w.text(), data.toString());
  }
});
test("historical pages join split characters without newlines or duplication", () => {
  const data = Buffer.from("A🦀BéC");
  for (let split = 1; split < data.length; split++) {
    const w = new ByteWindow();
    w.replace(chunk(data.subarray(split), split));
    w.prepend(chunk(data.subarray(0, split), 0));
    assert.equal(w.text(), data.toString());
  }
});
test("giant lines and bursts obey byte and line bounds", () => {
  const w = new ByteWindow(256, 10);
  w.replace(chunk(Buffer.from("x".repeat(10000)), 0));
  assert.equal(w.bytes.length, 256);
  assert.equal(w.start, 9744n);
  w.append(chunk(Buffer.from("a\n".repeat(100)), 10000));
  assert.ok(w.bytes.length <= 256);
  assert.ok(w.text().split("\n").length <= 11);
  assert.ok(w.evicted);
});
test("gaps, duplicate chunks and generations cannot merge", () => {
  const w = new ByteWindow();
  w.replace(chunk(Buffer.from("abc"), 0));
  assert.throws(() => w.append(chunk(Buffer.from("d"), 4)));
  assert.throws(() => w.append(chunk(Buffer.from("abc"), 0)));
  assert.throws(() =>
    w.append({ ...chunk(Buffer.from("d"), 3), generation: "two" }),
  );
});
test("invalid UTF-8 is flagged, incomplete valid tail is retained", () => {
  const w = new ByteWindow();
  w.replace(chunk(Buffer.from([0xc3]), 0));
  assert.equal(w.invalidUtf8(), false);
  assert.equal(w.text(), "");
  w.append(chunk(Buffer.from([0xa9]), 1));
  assert.equal(w.text(), "é");
  w.append(chunk(Buffer.from([0xff]), 2));
  assert.equal(w.invalidUtf8(), true);
});
