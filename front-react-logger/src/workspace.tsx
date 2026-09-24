import { useState, useSyncExternalStore } from "react";
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
export function Workspace({ store }: { store: ViewerStore }) {
  const state = useSyncExternalStore(
    store.subscribe,
    store.getSnapshot,
    store.getSnapshot,
  );
  const [mode, setMode] = useState<SourceMode>(() =>
    preference("local-logs-mode", "rlogger") === "external"
      ? "external"
      : "rlogger",
  );
  const [directories, setDirectories] = useState<Record<SourceMode, string>>(
    () => ({
      rlogger: preference("local-logs-path-rlogger", ""),
      external: preference(
        "local-logs-path-external",
        preference("local-logs-path", ""),
      ),
    }),
  );
  const directory = directories[mode];
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
        <p className="source-menu-title">Sources</p>
        <button
          type="button"
          className={`source-menu-item ${mode === "rlogger" ? "active" : ""}`}
          aria-current={mode === "rlogger" ? "page" : undefined}
          onClick={() => selectMode("rlogger")}
        >
          <span>Journaux RLOGGER</span>
          <small>Racine privée · maintenance</small>
        </button>
        <button
          type="button"
          className={`source-menu-item ${mode === "external" ? "active" : ""}`}
          aria-current={mode === "external" ? "page" : undefined}
          onClick={() => selectMode("external")}
        >
          <span>Journaux externes</span>
          <small>Application tierce · sans maintenance</small>
        </button>
      </nav>
      <div className="app-content">
        <header className="app-header">
          <h1>Local Logs</h1>
          <span>
            {mode === "rlogger" ? "Journaux RLOGGER" : "Journaux externes"}
          </span>
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
                localStorage.removeItem("local-logs-width");
              } catch {}
              setDirectories({ rlogger: "", external: "" });
              setWidth(280);
              selectMode("rlogger");
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
