import { LIMITS, offset, type Chunk } from "./protocol.ts";
/** Raw bytes remain the source of truth. Decoder retains incomplete UTF-8 tails. */
export class ByteWindow {
  bytes = new Uint8Array();
  start = 0n;
  end = 0n;
  size = 0n;
  generation = "";
  evicted = false;
  constructor(
    readonly maxBytes: number = LIMITS.viewBytes,
    readonly maxLines: number = LIMITS.viewLines,
  ) {}
  replace(chunk: Chunk) {
    this.bytes = decode64(chunk.data);
    this.start = offset(chunk.start);
    this.end = offset(chunk.end);
    this.size = offset(chunk.size);
    this.generation = chunk.generation;
    this.evicted = false;
    this.verify();
    this.trim("start");
  }
  append(chunk: Chunk) {
    if (
      chunk.generation !== this.generation ||
      offset(chunk.start) !== this.end
    )
      throw Error("Discontinuité de lecture");
    const data = decode64(chunk.data);
    if (BigInt(data.length) !== offset(chunk.end) - offset(chunk.start))
      throw Error("Intervalle incorrect");
    this.bytes = concat(this.bytes, data);
    this.end = offset(chunk.end);
    this.size = offset(chunk.size);
    this.trim("start");
  }
  prepend(chunk: Chunk) {
    if (
      chunk.generation !== this.generation ||
      offset(chunk.end) !== this.start
    )
      throw Error("Historique non contigu");
    this.bytes = concat(decode64(chunk.data), this.bytes);
    this.start = offset(chunk.start);
    this.verify();
    this.trim("end");
  }
  private verify() {
    if (BigInt(this.bytes.length) !== this.end - this.start)
      throw Error("Intervalle incorrect");
  }
  private trim(side: "start" | "end") {
    let boundary =
      side === "start"
        ? Math.max(0, this.bytes.length - this.maxBytes)
        : Math.min(this.maxBytes, this.bytes.length);
    let lines = 0;
    if (side === "start") {
      for (let i = this.bytes.length - 1; i >= boundary; i--) {
        if (this.bytes[i] === 10 && ++lines > this.maxLines) {
          boundary = i + 1;
          break;
        }
      }
      if (boundary) {
        this.bytes = this.bytes.slice(boundary);
        this.start += BigInt(boundary);
        this.evicted = true;
      }
    } else {
      for (let i = 0; i < boundary; i++) {
        if (this.bytes[i] === 10 && ++lines >= this.maxLines) {
          boundary = i + 1;
          break;
        }
      }
      if (boundary < this.bytes.length) {
        this.bytes = this.bytes.slice(0, boundary);
        this.end = this.start + BigInt(boundary);
        this.evicted = true;
      }
    }
  }
  text() {
    let begin = 0;
    if (this.start > 0n)
      while (
        begin < 3 &&
        begin < this.bytes.length &&
        (this.bytes[begin]! & 0xc0) === 0x80
      )
        begin++;
    return new TextDecoder("utf-8").decode(this.bytes.subarray(begin), {
      stream: true,
    });
  }
  invalidUtf8() {
    let begin = 0;
    if (this.start > 0n)
      while (
        begin < 3 &&
        begin < this.bytes.length &&
        (this.bytes[begin]! & 0xc0) === 0x80
      )
        begin++;
    try {
      new TextDecoder("utf-8", { fatal: true }).decode(
        this.bytes.subarray(begin),
        { stream: true },
      );
      return false;
    } catch {
      return true;
    }
  }
}
export function decode64(text: string) {
  const binary = atob(text);
  return Uint8Array.from(binary, (c) => c.charCodeAt(0));
}
function concat(a: Uint8Array, b: Uint8Array) {
  const joined = new Uint8Array(a.length + b.length);
  joined.set(a);
  joined.set(b, a.length);
  return joined;
}
