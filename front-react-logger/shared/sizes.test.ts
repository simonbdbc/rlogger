import assert from "node:assert/strict";
import { test } from "node:test";
import { formatBytes } from "../src/log-viewer/sizes";

test("sizes preserve large integer precision and distinguish unavailable from zero", () => {
  assert.equal(formatBytes("0"), "0 o");
  assert.equal(formatBytes(null), "indisponible");
  assert.equal(formatBytes(undefined), "indisponible");
  assert.equal(formatBytes("1024"), "1,0 Kio");
  assert.equal(formatBytes("1152921504606846976"), "1,0 Eio");
  assert.equal(formatBytes("9007199254740993"), "8,0 Pio");
});
