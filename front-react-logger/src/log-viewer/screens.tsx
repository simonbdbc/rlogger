import { useEffect, useId, useState, useSyncExternalStore } from "react";
import type { CSSProperties } from "react";
import {
  clearDirectoryPreference,
  clearTreeWidthPreference,
  directoryPreference,
  saveDirectoryPreference,
  saveTreeWidthPreference,
  treeWidthPreference,
} from "./preferences";
import { Reader } from "./reader";
import { Tree } from "./tree";
import { Action, GluestackUIProvider } from "./ui";
import { ViewerStore, type SourceMode } from "./viewer-store";
import "./viewer.css";

export interface LogsScreenProps {
  /** Optional first path; otherwise the last path for this source is reopened. */
  initialDirectory?: string;
  /** Reports edits to the path field so a host can preserve the text across route changes. */
  onDirectoryChange?: (path: string) => void;
  /** Called after the screen has cleared both saved paths and tree width. */
  onResetPreferences?: () => void;
}

function LogsScreen({
  mode,
  initialDirectory,
  onDirectoryChange,
  onResetPreferences,
}: LogsScreenProps & { mode: SourceMode }) {
  const [directory, setDirectory] = useState(
    () => initialDirectory ?? directoryPreference(mode),
  );
  const [store, setStore] = useState<ViewerStore | null>(null);
  useEffect(() => {
    const next = new ViewerStore();
    setStore(next);
    if (directory.trim()) void next.open(directory, mode);
    else void next.initialize();
    const close = () => next.stop();
    window.addEventListener("pagehide", close);
    return () => {
      window.removeEventListener("pagehide", close);
      next.stop();
    };
    // The initial directory is deliberately read only once per mounted screen.
  }, [mode]);

  return (
    <GluestackUIProvider>
      <div className="rlogger-viewer">
        {store ? (
          <LogsScreenContent
            mode={mode}
            store={store}
            directory={directory}
            setDirectory={setDirectory}
            onDirectoryChange={onDirectoryChange}
            onResetPreferences={onResetPreferences}
          />
        ) : (
          <div className="startup">Démarrage du lecteur local…</div>
        )}
      </div>
    </GluestackUIProvider>
  );
}

function LogsScreenContent({
  mode,
  store,
  directory,
  setDirectory,
  onDirectoryChange,
  onResetPreferences,
}: {
  mode: SourceMode;
  store: ViewerStore;
  directory: string;
  setDirectory: (path: string) => void;
  onDirectoryChange?: (path: string) => void;
  onResetPreferences?: () => void;
}) {
  const state = useSyncExternalStore(
    store.subscribe,
    store.getSnapshot,
    store.getSnapshot,
  );
  const directoryId = useId();
  const [width, setWidth] = useState(treeWidthPreference);
  const openedPath = state.root?.absolutePath;
  useEffect(() => {
    if (!openedPath) return;
    saveDirectoryPreference(mode, openedPath);
  }, [mode, openedPath]);

  const resize = (next: number) => {
    const value = Math.max(200, Math.min(600, next));
    setWidth(value);
    saveTreeWidthPreference(value);
  };
  const reset = () => {
    clearDirectoryPreference("rlogger");
    clearDirectoryPreference("external");
    clearTreeWidthPreference();
    setDirectory("");
    onDirectoryChange?.("");
    setWidth(280);
    void store.closeRoot();
    onResetPreferences?.();
  };

  return (
    <div className="app-content">
      <header className="app-header">
        <div className="source-summary">
          <h3>
            {mode === "rlogger" ? "Journaux RLOGGER" : "Journaux externes"}
          </h3>
          <small>
            {mode === "rlogger" ? "maintenance auto" : "sans maintenance"}
          </small>
        </div>
      </header>
      <form
        className="path-toolbar"
        onSubmit={(event) => {
          event.preventDefault();
          void store.open(directory, mode);
        }}
      >
        <label htmlFor={directoryId}>Dossier local</label>
        <input
          id={directoryId}
          placeholder={
            mode === "rlogger"
              ? "/chemin/vers/la/racine/rlogger"
              : "/chemin/absolu/vers/les/logs"
          }
          value={directory}
          onChange={(event) => {
            setDirectory(event.target.value);
            onDirectoryChange?.(event.target.value);
          }}
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
        style={{ "--tree-width": `${width}px` } as CSSProperties}
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
          onKeyDown={(event) => {
            if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
              event.preventDefault();
              resize(width + (event.key === "ArrowRight" ? 20 : -20));
            }
          }}
          onPointerDown={(event) => {
            event.currentTarget.setPointerCapture(event.pointerId);
          }}
          onPointerMove={(event) => {
            if (event.currentTarget.hasPointerCapture(event.pointerId)) {
              const left =
                event.currentTarget.parentElement?.getBoundingClientRect()
                  .left ?? 0;
              resize(event.clientX - left);
            }
          }}
          onPointerUp={(event) => {
            if (event.currentTarget.hasPointerCapture(event.pointerId)) {
              event.currentTarget.releasePointerCapture(event.pointerId);
            }
          }}
        >
          <span>Ⅱ</span>
        </div>
        <Reader store={store} state={state} />
      </div>
      <footer className="footer">
        <span>Texte brut · Fichiers locaux</span>
        <button className="reset-preferences" onClick={reset}>
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
  );
}

export function RloggerLogsScreen(props: LogsScreenProps) {
  return <LogsScreen mode="rlogger" {...props} />;
}

export function ExternalLogsScreen(props: LogsScreenProps) {
  return <LogsScreen mode="external" {...props} />;
}
