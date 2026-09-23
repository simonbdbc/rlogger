import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Action } from "./ui";
import type { ViewerStore, ViewState } from "./viewer-store";
const HEIGHT = 22;
export function Reader({
  store,
  state,
}: {
  store: ViewerStore;
  state: ViewState;
}) {
  const scroll = useRef<HTMLDivElement>(null);
  const [top, setTop] = useState(0);
  const [height, setHeight] = useState(600);
  const anchor = useRef<{ line: number; top: number } | null>(null);
  const lines = useMemo(() => state.text.split("\n"), [state.text]);
  const count = state.text ? lines.length : 0;
  const first = Math.max(0, Math.floor(top / HEIGHT) - 8);
  const last = Math.min(count, first + Math.ceil(height / HEIGHT) + 16);
  const width = useMemo(
    () =>
      Math.max(0, ...lines.map((line) => Math.min(line.length, 1024 * 1024))) *
        8.5 +
      48,
    [lines],
  );
  useEffect(() => {
    if (!scroll.current) return;
    const observer = new ResizeObserver((entries) =>
      setHeight(entries[0]!.contentRect.height),
    );
    observer.observe(scroll.current);
    return () => observer.disconnect();
  }, []);
  useLayoutEffect(() => {
    const el = scroll.current;
    if (!el) return;
    if (anchor.current && !state.loading) {
      el.scrollTop = anchor.current.top + state.prependedRows * HEIGHT;
      anchor.current = null;
    } else if (state.following) el.scrollTop = el.scrollHeight;
  }, [state.text, state.following, state.loading, state.prependedRows]);
  const older = () => {
    anchor.current = {
      line: lines.length,
      top: scroll.current?.scrollTop ?? 0,
    };
    void store.older();
  };
  return (
    <section className="reader" aria-label="Lecteur de fichier">
      <div className="reader-heading">
        <h2>{state.selected?.name ?? "Lecteur de logs"}</h2>
        <span
          className={`connection ${state.connected && state.selected ? "live" : ""}`}
          role="status"
        >
          <i />
          {state.status}
        </span>
      </div>
      {state.selected &&
        (state.selected.state === "recovered" || state.selected.reason) && (
          <div className="file-notice">
            {state.selected.state === "recovered"
              ? "Récupéré après interruption · buffers non récupérables. "
              : ""}
            {state.selected.reason}
          </div>
        )}
      <div className="reader-toolbar">
        <Action
          dark
          disabled={!state.selected || state.start === "0" || state.loading}
          onPress={older}
        >
          Charger plus ancien
        </Action>
        <span>
          {state.selected
            ? `${state.start} – ${state.end} octets`
            : "Texte brut"}
        </span>
      </div>
      {state.error ? (
        <div className="reader-alert" role="alert">
          {state.error}
          {state.selected && state.status === "Fichier indisponible" ? (
            <Action dark onPress={() => void store.select(state.selected!)}>
              Réessayer
            </Action>
          ) : null}
        </div>
      ) : null}
      {state.evicted || state.start !== "0" ? (
        <div className="window-note">
          Fenêtre partielle · historique disponible sur disque
          {state.invalidUtf8 ? " · UTF-8 invalide remplacé par �" : ""}
        </div>
      ) : state.invalidUtf8 ? (
        <div className="window-note">UTF-8 invalide remplacé par �</div>
      ) : null}
      <div
        className="log-scroll"
        ref={scroll}
        tabIndex={0}
        aria-label="Contenu du fichier"
        onScroll={(e) => {
          const el = e.currentTarget;
          setTop(el.scrollTop);
          const bottom = el.scrollHeight - el.scrollTop - el.clientHeight <= 5;
          if (bottom !== state.following) store.setFollowing(bottom);
        }}
      >
        {count > 0 ? (
          <div
            className="log-canvas"
            style={{ height: count * HEIGHT, minWidth: width }}
          >
            <pre className="log-lines" style={{ top: first * HEIGHT }}>
              {lines.slice(first, last).join("\n")}
            </pre>
          </div>
        ) : (
          <div className="reader-empty">
            {state.loading
              ? "Lecture du fichier…"
              : state.selected
                ? "Ce fichier est vide. Les prochains ajouts apparaîtront ici."
                : "Choisissez un fichier dans l’arborescence."}
          </div>
        )}
      </div>
    </section>
  );
}
