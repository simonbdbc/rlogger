import { useEffect, useState } from "react";
import { ExternalLogsScreen, RloggerLogsScreen } from "./log-viewer";
import type { SourceMode } from "./log-viewer/viewer-store";

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

export function Workspace() {
  const [initial] = useState(sourcePreferences);
  const [mode, setMode] = useState<SourceMode>(initial.mode);
  const [directories, setDirectories] = useState(initial.directories);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (initial.migrate) {
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
    }
    setReady(true);
  }, [initial]);

  const selectMode = (next: SourceMode) => {
    if (next === mode) return;
    setMode(next);
    try {
      localStorage.setItem("local-logs-mode", next);
    } catch {}
  };

  const resetPreferences = () => {
    try {
      for (const key of [
        "local-logs-path",
        "local-logs-path-rlogger",
        "local-logs-path-external",
        "local-logs-mode",
        "local-logs-source-migration-v2",
        "local-logs-width",
      ]) {
        localStorage.removeItem(key);
      }
    } catch {}
    setDirectories({ rlogger: "", external: "" });
    setMode("rlogger");
  };

  const changeDirectory = (path: string) => {
    setDirectories((current) => ({ ...current, [mode]: path }));
  };

  return (
    <main className="standalone-shell">
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
      {ready ? (
        mode === "rlogger" ? (
          <RloggerLogsScreen
            key="rlogger"
            initialDirectory={directories.rlogger}
            onDirectoryChange={changeDirectory}
            onResetPreferences={resetPreferences}
          />
        ) : (
          <ExternalLogsScreen
            key="external"
            initialDirectory={directories.external}
            onDirectoryChange={changeDirectory}
            onResetPreferences={resetPreferences}
          />
        )
      ) : (
        <div className="startup">Démarrage du lecteur local…</div>
      )}
    </main>
  );
}
