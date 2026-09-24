import type { SourceMode } from "./viewer-store";

function read(key: string, fallback: string) {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {}
}

function remove(key: string) {
  try {
    localStorage.removeItem(key);
  } catch {}
}

export function directoryPreference(mode: SourceMode) {
  return read(`local-logs-path-${mode}`, "");
}

export function saveDirectoryPreference(mode: SourceMode, path: string) {
  write(`local-logs-path-${mode}`, path);
}

export function clearDirectoryPreference(mode: SourceMode) {
  remove(`local-logs-path-${mode}`);
}

export function treeWidthPreference() {
  const width = Number(read("local-logs-width", "280"));
  return width >= 200 && width <= 600 ? width : 280;
}

export function saveTreeWidthPreference(width: number) {
  write("local-logs-width", String(width));
}

export function clearTreeWidthPreference() {
  remove("local-logs-width");
}
