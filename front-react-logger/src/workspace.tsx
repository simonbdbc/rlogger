import { useState, useSyncExternalStore } from "react";
import type { ViewerStore } from "./viewer-store";
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
  const [directory, setDirectory] = useState(() =>
    preference("local-logs-path", ""),
  );
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
  return (
    <main className="app-shell">
      <header className="app-header">
        <h1>Local Logs</h1>
        <span>Lecture locale</span>
      </header>
      <form
        className="path-toolbar"
        onSubmit={(e) => {
          e.preventDefault();
          void store.open(directory);
        }}
      >
        <label htmlFor="directory">Dossier local</label>
        <input
          id="directory"
          placeholder="/chemin/absolu/vers/les/logs"
          value={directory}
          onChange={(e) => setDirectory(e.target.value)}
          autoComplete="off"
          spellCheck={false}
        />
        <Action
          primary
          disabled={state.loading || !directory.trim()}
          onPress={() => void store.open(directory)}
        >
          Ouvrir
        </Action>
      </form>
      {state.root?.managedReason ? (
        <p className="root-notice" role="status">
          Maintenance automatique RLOGGER indisponible. Téléchargement et
          suppression restent disponibles.
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
              localStorage.removeItem("local-logs-width");
            } catch {}
            setDirectory("");
            setWidth(280);
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
    </main>
  );
}
