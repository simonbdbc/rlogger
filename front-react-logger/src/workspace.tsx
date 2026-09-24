import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { SourceMode, ViewerStore } from "./viewer-store";
import { Action } from "./ui";
import { Tree } from "./tree";
import { Reader } from "./reader";
function preference(key: string, fallback: string) {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}
function isRloggerRoot(path: string) {
  return path.replace(/\/+$/, "").split("/").at(-1) === "rlogger";
}
function sourcePreferences() {
  const legacy = preference("local-logs-path", "");
  let rlogger = preference("local-logs-path-rlogger", "");
  let external = preference("local-logs-path-external", legacy);
  let mode: SourceMode =
    preference("local-logs-mode", "rlogger") === "external"
      ? "external"
      : "rlogger";
  const migrate = preference("local-logs-source-migration-v2", "") !== "done";
  if (migrate) {
    if (!rlogger && isRloggerRoot(external)) {
      rlogger = external;
      external = legacy && !isRloggerRoot(legacy) ? legacy : "";
      if (mode === "external") mode = "rlogger";
    } else if (!rlogger && isRloggerRoot(legacy)) {
      rlogger = legacy;
    }
  }
  return { mode, directories: { rlogger, external }, migrate };
}
export function Workspace({ store }: { store: ViewerStore }) {
  const state = useSyncExternalStore(
    store.subscribe,
    store.getSnapshot,
    store.getSnapshot,
  );
  const [initial] = useState(sourcePreferences);
  const [mode, setMode] = useState<SourceMode>(initial.mode);
  const [directories, setDirectories] = useState<Record<SourceMode, string>>(
    initial.directories,
  );
  const directory = directories[mode];
  const openedMode = useRef<{ store: ViewerStore; mode: SourceMode } | null>(
    null,
  );
  useEffect(() => {
    if (!initial.migrate) return;
    try {
      localStorage.setItem(
        "local-logs-path-rlogger",
        initial.directories.rlogger,
      );
      localStorage.setItem(
        "local-logs-path-external",
        initial.directories.external,
      );
      localStorage.setItem("local-logs-mode", initial.mode);
      localStorage.setItem("local-logs-source-migration-v2", "done");
      localStorage.removeItem("local-logs-path");
    } catch {}
  }, [initial]);
  useEffect(() => {
    if (openedMode.current?.store === store && openedMode.current.mode === mode)
      return;
    openedMode.current = { store, mode };
    if (directory.trim()) void store.open(directory, mode);
  }, [store, mode, directory]);
  const [width, setWidth] = useState(() => {
    const width = Number(preference("local-logs-width", "280"));
    return width >= 200 && width <= 600 ? width : 280;
  });
  const resize = (next: number) => {
    const value = Math.max(200, Math.min(600, next));
    setWidth(value);
    try {
      localStorage.setItem("local-logs-width", String(value));
    } catch {}
  };
  const selectMode = (next: SourceMode) => {
    if (next === mode) return;
    setMode(next);
    try {
      localStorage.setItem("local-logs-mode", next);
    } catch {}
    void store.closeRoot();
  };
  return (
    <main className="app-shell">
      <nav className="source-menu" aria-label="Sources de journaux">
        <h1>
          RLOG <small>local</small>
        </h1>
        <p className="source-menu-title">Menu Sources</p>
        <button
          type="button"
          className={`source-menu-item ${mode === "rlogger" ? "active" : ""}`}
          aria-current={mode === "rlogger" ? "page" : undefined}
          onClick={() => selectMode("rlogger")}
        >
          <span>Journaux RLOGGER</span>
        </button>
        <button
          type="button"
          className={`source-menu-item ${mode === "external" ? "active" : ""}`}
          aria-current={mode === "external" ? "page" : undefined}
          onClick={() => selectMode("external")}
        >
          <span>Journaux externes</span>
        </button>
      </nav>
      <div className="app-content">
        <header className="app-header">
          {mode === "rlogger" ? (
            <div className="source-summary">
              <h3>Journaux RLOGGER</h3>
              <small>maintenance auto</small>
            </div>
          ) : (
            <div className="source-summary">
              <h3>Journaux externes</h3>
              <small>sans maintenance</small>
            </div>
          )}
        </header>
        <form
          className="path-toolbar"
          onSubmit={(e) => {
            e.preventDefault();
            void store.open(directory, mode);
          }}
        >
          <label htmlFor="directory">Dossier local</label>
          <input
            id="directory"
            placeholder={
              mode === "rlogger"
                ? "/chemin/vers/la/racine/rlogger"
                : "/chemin/absolu/vers/les/logs"
            }
            value={directory}
            onChange={(e) =>
              setDirectories((current) => ({
                ...current,
                [mode]: e.target.value,
              }))
            }
            autoComplete="off"
            spellCheck={false}
          />
          <Action
            primary
            disabled={state.loading || !directory.trim()}
            onPress={() => void store.open(directory, mode)}
          >
            Ouvrir
          </Action>
        </form>
        {mode === "rlogger" && state.root?.managedReason ? (
          <p className="root-notice" role="status">
            Maintenance automatique RLOGGER indisponible. Téléchargement et
            suppression restent disponibles. {state.root.managedReason}
          </p>
        ) : null}
        <div
          className="workspace"
          style={{ "--tree-width": `${width}px` } as React.CSSProperties}
        >
          <Tree store={store} state={state} />
          <div
            className="divider"
            role="separator"
            aria-label="Largeur de l’arbre"
            aria-orientation="vertical"
            aria-valuenow={width}
            aria-valuemin={200}
            aria-valuemax={600}
            tabIndex={0}
            onKeyDown={(e) => {
              if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
                e.preventDefault();
                resize(width + (e.key === "ArrowRight" ? 20 : -20));
              }
            }}
            onPointerDown={(e) => {
              e.currentTarget.setPointerCapture(e.pointerId);
            }}
            onPointerMove={(e) => {
              if (e.currentTarget.hasPointerCapture(e.pointerId))
                resize(e.clientX);
            }}
            onPointerUp={(e) =>
              e.currentTarget.releasePointerCapture(e.pointerId)
            }
          >
            <span>Ⅱ</span>
          </div>
          <Reader store={store} state={state} />
        </div>
        <footer className="footer">
          <span>Texte brut · Fichiers locaux</span>
          <button
            className="reset-preferences"
            onClick={() => {
              try {
                localStorage.removeItem("local-logs-path");
                localStorage.removeItem("local-logs-path-rlogger");
                localStorage.removeItem("local-logs-path-external");
                localStorage.removeItem("local-logs-mode");
                localStorage.removeItem("local-logs-source-migration-v2");
                localStorage.removeItem("local-logs-width");
              } catch {}
              setDirectories({ rlogger: "", external: "" });
              setWidth(280);
              setMode("rlogger");
              openedMode.current = { store, mode: "rlogger" };
              void store.closeRoot();
            }}
          >
            Réinitialiser les préférences
          </button>
          <div className="footer-right">
            {state.newer ? <span>Nouveaux octets disponibles</span> : null}
            <Action
              disabled={!state.selected}
              onPress={() => void store.bottom()}
            >
              ↓ Retour en bas
            </Action>
          </div>
        </footer>
      </div>
    </main>
  );
}
