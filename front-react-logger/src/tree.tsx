import React from "react";
import { Action } from "./ui";
import { Sizes } from "./sizes";
import { EntryActions } from "./entry-actions";
import type { ViewerStore, ViewState } from "./viewer-store";
export function Tree({
  store,
  state,
}: {
  store: ViewerStore;
  state: ViewState;
}) {
  function branch(id: string, depth: number): React.ReactNode {
    const page = state.branches.get(id);
    const error = state.treeErrors.get(id);
    if (error)
      return (
        <div className="tree-message" role="alert">
          {error}
          <Action onPress={() => void store.loadBranch(id)}>Réessayer</Action>
        </div>
      );
    if (!page) return <p className="tree-message">Chargement…</p>;
    return (
      <>
        {page.entries.map((entry) => (
          <React.Fragment key={entry.id}>
            <div className="tree-entry">
              <button
                type="button"
                role="treeitem"
                aria-label={entry.name}
                aria-level={depth + 1}
                aria-expanded={
                  entry.kind === "directory"
                    ? state.opened.has(entry.id)
                    : undefined
                }
                aria-selected={state.selected?.id === entry.id}
                className={`tree-row ${state.selected?.id === entry.id ? "selected" : ""}`}
                style={{ paddingLeft: 16 + depth * 20 }}
                title={entry.relativePath}
                onClick={() =>
                  entry.kind === "directory"
                    ? void store.toggle(entry)
                    : void store.select(entry)
                }
                onKeyDown={(event) => {
                  if (
                    entry.kind === "directory" &&
                    ((event.key === "ArrowRight" &&
                      !state.opened.has(entry.id)) ||
                      (event.key === "ArrowLeft" && state.opened.has(entry.id)))
                  ) {
                    event.preventDefault();
                    void store.toggle(entry);
                  }
                }}
              >
                <span className="chevron" aria-hidden="true">
                  {entry.kind === "directory"
                    ? state.opened.has(entry.id)
                      ? "⌄"
                      : "›"
                    : ""}
                </span>
                <FileIcon directory={entry.kind === "directory"} />
                <span className="entry-name">{entry.name}</span>
                <Sizes
                  loading={!state.inventory || state.inventory.busy}
                  size={
                    state.inventory?.items[entry.relativePath] ??
                    (entry.kind === "file"
                      ? {
                          content: entry.size,
                          allocated: entry.allocatedSize,
                          partial: false,
                        }
                      : undefined)
                  }
                />
              </button>
              <EntryActions
                entry={entry}
                store={store}
                busy={state.actionBusy}
              />
            </div>
            {entry.kind === "directory" && state.opened.has(entry.id) ? (
              <div role="group">{branch(entry.id, depth + 1)}</div>
            ) : null}
          </React.Fragment>
        ))}
        {page.entries.length === 0 ? (
          <p className="tree-message">Aucun fichier ou dossier visible.</p>
        ) : null}
        {page.cursor ? (
          <div className="more-tree">
            <Action onPress={() => void store.loadBranch(id, true)}>
              Afficher la suite
            </Action>
          </div>
        ) : null}
      </>
    );
  }
  return (
    <aside className="tree-panel" aria-label="Fichiers locaux">
      <div className="tree-toolbar">
        <h2>Fichiers</h2>
        <Action disabled={!state.root} onPress={() => void store.refresh()}>
          ↻ Actualiser
        </Action>
      </div>
      {state.root && (
        <div className="root-sizes">
          <Sizes
            size={state.inventory?.items[""]}
            loading={!state.inventory || state.inventory.busy}
          />
          <small>
            {state.inventory?.sampledAt
              ? `Relevé ${new Date(state.inventory.sampledAt).toLocaleTimeString()}${state.inventory.partial ? " · partiel" : ""}${state.inventory.busy ? " · calcul en cours" : ""}`
              : "Inventaire en cours"}
          </small>
          <small>
            Estimation ; les clones et snapshots peuvent modifier l’espace
            libérable.
          </small>
          {state.inventory?.notices.map((s, i) => (
            <small key={i}>{s}</small>
          ))}
        </div>
      )}
      {state.root ? (
        <div
          className="tree-scroll"
          role="tree"
          aria-label="Arborescence du dossier"
          onKeyDown={(e) => {
            if (
              !(e.target instanceof Element) ||
              !e.target.closest('[role="treeitem"]')
            )
              return;
            if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key))
              return;
            const rows = Array.from(
              e.currentTarget.querySelectorAll<HTMLButtonElement>(
                "[role=treeitem]",
              ),
            );
            const index = rows.indexOf(
              document.activeElement as HTMLButtonElement,
            );
            const next =
              e.key === "Home"
                ? 0
                : e.key === "End"
                  ? rows.length - 1
                  : Math.min(
                      rows.length - 1,
                      Math.max(0, index + (e.key === "ArrowDown" ? 1 : -1)),
                    );
            e.preventDefault();
            rows[next]?.focus();
          }}
        >
          <p className="root-caption" title={state.root.absolutePath}>
            {state.root.absolutePath.split("/").filter(Boolean).at(-1) ?? "/"}
          </p>
          {branch(state.root.nodeId, 0)}
        </div>
      ) : (
        <div className="tree-message empty-tree">
          Ouvrez un dossier pour explorer ses fichiers.
          <br />
          <br />
          Les fichiers restent sur cette machine.
        </div>
      )}
    </aside>
  );
}
function FileIcon({ directory }: { directory: boolean }) {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      aria-hidden="true"
      fill={directory ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth="1.6"
    >
      {directory ? (
        <path d="M3 5h7l2 3h9v12H3z" />
      ) : (
        <>
          <path d="M6 3h8l5 5v13H6z" />
          <path d="M14 3v6h5" />
        </>
      )}
    </svg>
  );
}
