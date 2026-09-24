import { useEffect, useRef, useState } from "react";
import type { Entry } from "./protocol";
import type { ViewerStore } from "./viewer-store";
import { Action } from "./ui";

export function EntryActions({
  entry,
  store,
  busy,
}: {
  entry: Entry;
  store: ViewerStore;
  busy: boolean;
}) {
  const [confirm, setConfirm] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const deleteButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const element = dialog.current;
    if (!element || !confirm) return;
    const previous = deleteButton.current;
    element.showModal();
    return () => {
      element.close();
      if (previous?.isConnected) previous.focus();
    };
  }, [confirm, entry.id]);
  const directory = entry.kind === "directory";
  const target = `${directory ? "le dossier" : "le fichier"} ${entry.name}`;
  return (
    <div className="entry-actions">
      <button
        type="button"
        className="entry-icon-action"
        aria-label={`Télécharger ${target}`}
        title={entry.reason || `Télécharger ${target}`}
        disabled={!entry.eligible || busy}
        onClick={() => void store.download(entry)}
      >
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.7"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5" />
        </svg>
      </button>
      <button
        type="button"
        className="entry-icon-action delete-icon"
        ref={deleteButton}
        aria-label={`Supprimer ${target}`}
        title={entry.reason || `Supprimer ${target}`}
        disabled={!entry.eligible || busy}
        onClick={() => setConfirm(true)}
      >
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.7"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7m4-7v7" />
        </svg>
      </button>
      {confirm && (
        <dialog
          ref={dialog}
          role="alertdialog"
          aria-label="Confirmer la suppression"
          className="delete-confirm"
          onCancel={() => setConfirm(false)}
        >
          <p>Supprimer définitivement {entry.name} ?</p>
          <p>{entry.relativePath}</p>
          {directory && (
            <p>
              Tout le contenu du dossier sera supprimé, y compris les
              sous-dossiers et fichiers cachés.
            </p>
          )}
          {entry.state === "active" && (
            <p>Ce fichier est en cours d’écriture.</p>
          )}
          <Action dark onPress={() => setConfirm(false)}>
            Annuler
          </Action>
          <Action
            dark
            onPress={() => {
              setConfirm(false);
              void store.deleteFile(entry);
            }}
          >
            Confirmer la suppression
          </Action>
        </dialog>
      )}
    </div>
  );
}
