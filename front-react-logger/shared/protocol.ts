export const VERSION = 1 as const;
export const LIMITS = Object.freeze({
  sessions: 4,
  nodes: 10000,
  branches: 64,
  page: 500,
  snapshot: 256 * 1024,
  chunk: 64 * 1024,
  inFlight: 1024 * 1024,
  viewBytes: 1024 * 1024,
  viewLines: 20000,
  pollMs: 250,
  body: 8192,
  scanEntries: 200000,
});
export type ErrorCode =
  | "INVALID_REQUEST"
  | "FORBIDDEN"
  | "NOT_FOUND"
  | "PERMISSION"
  | "ROOT_CHANGED"
  | "NODE_EXPIRED"
  | "GENERATION_CHANGED"
  | "CURSOR_INVALID"
  | "LIMIT"
  | "SESSION_EXPIRED"
  | "FILE_PROTECTED"
  | "FILE_CHANGED"
  | "FILE_BUSY"
  | "PRECONDITION_REQUIRED"
  | "DOWNLOAD_EXPIRED"
  | "INTERNAL";
export class ProtocolError extends Error {
  constructor(
    public code: ErrorCode,
    message: string,
    public status = 400,
  ) {
    super(message);
  }
}
export interface Entry {
  id: string;
  name: string;
  relativePath: string;
  kind: "directory" | "file";
  size: string;
  identity: string;
  allocatedSize: string | null;
  state: "active" | "closed" | "recovered" | "legacy" | "unknown";
  eligible: boolean;
  reason: string;
}
export interface FileSize {
  content: string;
  allocated: string | null;
  partial: boolean;
}
export interface Inventory {
  items: Record<string, FileSize>;
  sampledAt: string;
  partial: boolean;
  busy: boolean;
  notices: string[];
}
export interface Root {
  rootId: string;
  nodeId: string;
  absolutePath: string;
  managedReason: string | null;
}
export interface EntryPage {
  entries: Entry[];
  revision: string;
  cursor: string | null;
}
export interface Chunk {
  generation: string;
  start: string;
  end: string;
  size: string;
  data: string;
  partialStart: boolean;
}
export interface Selection {
  rootId: string;
  fileId: string;
  revision: number;
  generation: string;
  offset: string;
}
export type ClientMessage =
  | ({ type: "subscribe" } & Selection)
  | { type: "ack"; revision: number; generation: string; end: string }
  | { type: "branches"; rootId: string; ids: string[] }
  | { type: "unsubscribe" };
export type ServerMessage =
  | { type: "file-renamed"; revision: number; entry: Entry }
  | ({ type: "chunk"; revision: number } & Chunk)
  | { type: "subscribed"; revision: number; generation: string; offset: string }
  | { type: "reset"; revision: number; reason: string }
  | { type: "file-unavailable"; revision: number; reason: string }
  | { type: "tree-changed"; ids: string[] }
  | { type: "heartbeat" }
  | { type: "resync-required"; revision: number; reason: string }
  | { type: "error"; code: ErrorCode; message: string };
export function offset(value: unknown): bigint {
  if (typeof value !== "string" || !/^\d{1,20}$/.test(value))
    throw new ProtocolError("CURSOR_INVALID", "Position d’octets invalide.");
  const n = BigInt(value);
  if (n > BigInt(Number.MAX_SAFE_INTEGER))
    throw new ProtocolError(
      "CURSOR_INVALID",
      "Position supérieure à la limite du système de lecture (2^53−1).",
    );
  return n;
}
