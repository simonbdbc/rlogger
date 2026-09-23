import { Api, ApiError } from "./api";
import { ByteWindow } from "../shared/window";
import {
  LIMITS,
  type Root,
  type Entry,
  type EntryPage,
  type Chunk,
  type ServerMessage,
  type ClientMessage,
  type Inventory,
} from "../shared/protocol";
export interface ViewState {
  inventory: Inventory | null;
  actionBusy: boolean;
  root: Root | null;
  selected: Entry | null;
  branches: Map<string, EntryPage>;
  opened: Set<string>;
  treeErrors: Map<string, string>;
  text: string;
  start: string;
  end: string;
  status: string;
  error: string;
  loading: boolean;
  following: boolean;
  newer: boolean;
  evicted: boolean;
  invalidUtf8: boolean;
  connected: boolean;
  version: number;
  prependedRows: number;
}
export class ViewerStore {
  private state: ViewState = {
    inventory: null,
    actionBusy: false,
    root: null,
    selected: null,
    branches: new Map(),
    opened: new Set(),
    treeErrors: new Map(),
    text: "",
    start: "0",
    end: "0",
    status: "Choisissez un dossier",
    error: "",
    loading: false,
    following: true,
    newer: false,
    evicted: false,
    invalidUtf8: false,
    connected: false,
    version: 0,
    prependedRows: 0,
  };
  private listeners = new Set<() => void>();
  readonly api = new Api();
  private bytes = new ByteWindow();
  private revision = 0;
  private rootRevision = 0;
  private cursor = "0";
  private socket?: WebSocket;
  private retry?: ReturnType<typeof setTimeout>;
  private heartbeat?: ReturnType<typeof setInterval>;
  private lastHeartbeat = 0;
  private stopped = false;
  private attempts = 0;
  private activeBranchLoads = new Set<string>();
  private pendingTree = new Set<string>();
  private initPromise?: Promise<void>;
  private inventoryTimer?: ReturnType<typeof setTimeout>;
  private inventoryLoading = false;
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  private update(patch: Partial<ViewState>) {
    this.state = { ...this.state, ...patch, version: this.state.version + 1 };
    for (const listener of this.listeners) listener();
  }
  private showBytes() {
    this.update({
      text: this.bytes.text(),
      start: String(this.bytes.start),
      end: String(this.bytes.end),
      evicted: this.bytes.evicted,
      invalidUtf8: this.bytes.invalidUtf8(),
    });
  }
  initialize() {
    if (!this.initPromise)
      this.initPromise = this.api
        .initialize()
        .then(() => {
          if (this.stopped) void this.api.close();
          else this.connect();
        })
        .catch((e) => {
          this.initPromise = undefined;
          this.update({ error: e.message, status: "Compagnon indisponible" });
        });
    return this.initPromise;
  }
  private send(message: ClientMessage) {
    if (this.socket?.readyState === WebSocket.OPEN)
      this.socket.send(JSON.stringify(message));
  }
  private observeBranches() {
    if (this.state.root)
      this.send({
        type: "branches",
        rootId: this.state.root.rootId,
        ids: [...this.state.opened].slice(0, LIMITS.branches),
      });
  }
  private subscribeFile() {
    const { root, selected } = this.state;
    if (root && selected && this.bytes.generation)
      this.send({
        type: "subscribe",
        rootId: root.rootId,
        fileId: selected.id,
        generation: this.bytes.generation,
        offset: this.cursor,
        revision: this.revision,
      });
  }
  private connect() {
    if (this.stopped) return;
    const socket = new WebSocket(
      `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}/api/v1/live`,
      `local-logs.v1.${this.api.token}`,
    );
    this.socket = socket;
    socket.onopen = () => {
      if (this.socket !== socket) return;
      this.attempts = 0;
      this.lastHeartbeat = Date.now();
      this.update({
        connected: true,
        error: "",
        status: this.state.selected ? "En direct" : "Choisissez un fichier",
      });
      this.observeBranches();
      this.subscribeFile();
      if (this.heartbeat) clearInterval(this.heartbeat);
      this.heartbeat = setInterval(() => {
        if (Date.now() - this.lastHeartbeat > 15000) socket.close();
      }, 5000);
    };
    socket.onmessage = (event) => {
      if (this.socket !== socket) return;
      this.lastHeartbeat = Date.now();
      try {
        this.message(JSON.parse(event.data));
      } catch (e) {
        this.update({
          error: (e as Error).message,
          status: "Resynchronisation",
        });
        if (this.state.selected) void this.select(this.state.selected);
      }
    };
    socket.onerror = () => {};
    socket.onclose = () => {
      if (this.socket !== socket || this.stopped) return;
      if (this.heartbeat) clearInterval(this.heartbeat);
      this.update({ connected: false, status: "Reconnexion…" });
      this.retry = setTimeout(
        () => void this.reconnect(),
        Math.min(5000, 500 * 2 ** this.attempts++),
      );
    };
  }
  private async reconnect() {
    if (this.stopped) return;
    try {
      const response = await fetch("/api/v1/health");
      const health = await response.json();
      const session = await fetch("/api/v1/session", {
        headers: { "x-local-session": this.api.token },
      });
      if (health.serverId !== this.api.serverId || session.status === 401) {
        const previous = this.state.root?.absolutePath;
        await this.api.initialize();
        this.bytes = new ByteWindow();
        this.revision++;
        this.update({
          root: null,
          selected: null,
          branches: new Map(),
          opened: new Set(),
          text: "",
          status: "Nouvelle session : racine revalidée",
        });
        this.connect();
        if (previous) await this.open(previous);
      } else this.connect();
    } catch {
      this.retry = setTimeout(
        () => void this.reconnect(),
        Math.min(5000, 500 * 2 ** this.attempts++),
      );
    }
  }
  private message(message: ServerMessage) {
    if (message.type === "heartbeat") return;
    if (message.type === "tree-changed") {
      for (const id of message.ids)
        if (this.state.opened.has(id)) {
          if (this.activeBranchLoads.has(id)) this.pendingTree.add(id);
          else void this.loadBranch(id);
        }
      return;
    }
    if (message.type === "error") {
      this.update({ error: message.message });
      return;
    }
    if ("revision" in message && message.revision !== this.revision) return;
    if (message.type === "file-renamed") {
      this.update({ selected: message.entry });
      void this.refresh();
      return;
    }
    if (message.type === "chunk") {
      if (
        message.generation !== this.bytes.generation ||
        message.start !== this.cursor
      )
        throw Error("Discontinuité : relecture du fichier.");
      if (this.state.following) {
        this.bytes.append(message);
        this.showBytes();
      } else this.update({ newer: true });
      this.cursor = message.end;
      this.send({
        type: "ack",
        revision: this.revision,
        generation: message.generation,
        end: message.end,
      });
    } else if (message.type === "reset") {
      this.update({
        error: message.reason,
        status: "Fichier remplacé ou tronqué",
      });
      if (this.state.selected) void this.select(this.state.selected, true);
    } else if (message.type === "file-unavailable") {
      this.update({ error: message.reason, status: "Fichier indisponible" });
    } else if (message.type === "resync-required") {
      this.update({ status: "Reprise du flux", error: message.reason });
      this.socket?.close();
    } else if (message.type === "subscribed")
      this.update({ status: "En direct" });
  }
  async open(absolutePath: string) {
    await this.initialize();
    const operation = ++this.rootRevision;
    this.update({ loading: true, error: "" });
    try {
      const root = await this.api.request<Root>("/api/v1/roots", {
        method: "POST",
        body: JSON.stringify({ absolutePath }),
      });
      if (operation !== this.rootRevision || this.stopped) return;
      this.revision++;
      this.send({ type: "unsubscribe" });
      this.bytes = new ByteWindow();
      this.cursor = "0";
      this.update({
        root,
        inventory: null,
        actionBusy: false,
        selected: null,
        branches: new Map(),
        opened: new Set([root.nodeId]),
        treeErrors: new Map(),
        text: "",
        start: "0",
        end: "0",
        newer: false,
        evicted: false,
        status: "Choisissez un fichier",
      });
      try {
        localStorage.setItem("local-logs-path", root.absolutePath);
      } catch {}
      await this.loadBranch(root.nodeId);
      void this.statistics(true);
      this.observeBranches();
    } catch (e) {
      if (operation === this.rootRevision)
        this.update({ error: (e as Error).message });
    } finally {
      if (operation === this.rootRevision) this.update({ loading: false });
    }
  }
  async loadBranch(id: string, more = false) {
    const root = this.state.root;
    if (!root || this.activeBranchLoads.has(id)) return;
    const rootId = root.rootId;
    const current = this.state.branches.get(id);
    this.activeBranchLoads.add(id);
    try {
      const page = await this.api.request<EntryPage>(
        `/api/v1/roots/${rootId}/entries?parentId=${id}${more && current?.cursor ? `&cursor=${encodeURIComponent(current.cursor)}` : ""}`,
      );
      if (this.state.root?.rootId !== rootId) return;
      const branches = new Map(this.state.branches);
      const entries =
        more && current ? [...current.entries, ...page.entries] : page.entries;
      branches.set(id, { ...page, entries });
      let total = [...branches.values()].reduce(
        (n, p) => n + p.entries.length,
        0,
      );
      for (const [key, value] of branches) {
        if (total <= LIMITS.nodes) break;
        if (key !== id && !this.state.opened.has(key)) {
          branches.delete(key);
          total -= value.entries.length;
        }
      }
      if (total > LIMITS.nodes)
        throw Error(
          "Limite de 10 000 entrées : repliez des dossiers puis actualisez.",
        );
      const errors = new Map(this.state.treeErrors);
      errors.delete(id);
      this.update({ branches, treeErrors: errors });
      const selected = entries.find(
        (entry) => entry.id === this.state.selected?.id,
      );
      if (selected) this.update({ selected });
    } catch (e) {
      if (this.state.root?.rootId === rootId) {
        const errors = new Map(this.state.treeErrors);
        errors.set(id, (e as Error).message);
        this.update({ treeErrors: errors });
      }
    } finally {
      this.activeBranchLoads.delete(id);
      if (this.pendingTree.delete(id)) void this.loadBranch(id);
    }
  }
  async toggle(entry: Entry) {
    const opened = new Set(this.state.opened);
    if (opened.has(entry.id)) {
      opened.delete(entry.id);
      const prefix = entry.relativePath + "/";
      for (const branch of this.state.branches.values())
        for (const node of branch.entries)
          if (node.relativePath.startsWith(prefix)) opened.delete(node.id);
    } else {
      if (opened.size >= LIMITS.branches) {
        this.update({ error: "Maximum de 64 dossiers ouverts." });
        return;
      }
      opened.add(entry.id);
    }
    this.update({ opened });
    this.observeBranches();
    if (opened.has(entry.id)) await this.loadBranch(entry.id);
  }
  async refresh() {
    for (const id of this.state.opened) await this.loadBranch(id);
    void this.statistics(true);
  }
  async statistics(force = false) {
    const root = this.state.root;
    if (!root || this.inventoryLoading || this.stopped) return;
    this.inventoryLoading = true;
    if (this.inventoryTimer) clearTimeout(this.inventoryTimer);
    try {
      const inventory = await this.api.request<Inventory>(
        `/api/v1/roots/${root.rootId}/statistics${force ? "?refresh=1" : ""}`,
      );
      if (this.state.root?.rootId !== root.rootId || this.stopped) return;
      this.update({ inventory });
      const selected = this.state.selected;
      if (selected) {
        const entry = await this.api.request<Entry>(
          `/api/v1/roots/${root.rootId}/files/${selected.id}`,
        );
        if (
          this.state.root?.rootId === root.rootId &&
          this.state.selected?.id === selected.id
        )
          this.update({ selected: entry });
      }
    } catch (e) {
      if (this.state.root?.rootId === root.rootId)
        this.update({ error: (e as Error).message });
    } finally {
      this.inventoryLoading = false;
      if (!this.stopped && this.state.root)
        this.inventoryTimer = setTimeout(
          () => void this.statistics(),
          this.state.inventory?.busy ? 500 : 5000,
        );
    }
  }
  async download(selected = this.state.selected) {
    const root = this.state.root;
    if (!root || !selected?.eligible || this.state.actionBusy) return;
    this.update({ actionBusy: true, error: "" });
    try {
      const { url } = await this.api.request<{ url: string }>(
        `/api/v1/roots/${root.rootId}/files/${selected.id}/download`,
        { method: "POST", headers: { "If-Match": selected.identity } },
      );
      if (this.state.root?.rootId !== root.rootId) return;
      const link = document.createElement("a");
      link.href = url;
      link.download =
        selected.name + (selected.kind === "directory" ? ".tar" : "");
      link.rel = "noreferrer";
      link.click();
      while (!this.stopped && this.state.root?.rootId === root.rootId) {
        await new Promise((resolve) => setTimeout(resolve, 500));
        const entry = await this.api.request<Entry>(
          `/api/v1/roots/${root.rootId}/files/${selected.id}`,
        );
        if (this.state.root?.rootId !== root.rootId) break;
        if (this.state.selected?.id === selected.id)
          this.update({ selected: entry });
        if (!entry.reason.includes("occupé")) break;
      }
    } catch (e) {
      if (this.state.root?.rootId === root.rootId)
        this.update({ error: (e as Error).message });
    } finally {
      if (this.state.root?.rootId === root.rootId)
        this.update({ actionBusy: false });
      void this.statistics();
    }
  }
  async deleteFile(selected = this.state.selected) {
    const root = this.state.root;
    if (!root || !selected?.eligible || this.state.actionBusy) return;
    this.update({ actionBusy: true, error: "" });
    try {
      await this.api.request(
        `/api/v1/roots/${root.rootId}/files/${selected.id}`,
        { method: "DELETE", headers: { "If-Match": selected.identity } },
      );
      if (this.state.root?.rootId !== root.rootId) return;
      if (
        this.state.selected?.id === selected.id ||
        this.state.selected?.relativePath.startsWith(
          selected.relativePath + "/",
        )
      ) {
        this.revision++;
        this.send({ type: "unsubscribe" });
        this.bytes = new ByteWindow();
        this.update({
          selected: null,
          text: "",
          start: "0",
          end: "0",
          status: "Fichier supprimé",
          following: true,
          newer: false,
          evicted: false,
          invalidUtf8: false,
          prependedRows: 0,
        });
      }
      const branches = new Map(this.state.branches);
      const opened = new Set(this.state.opened);
      for (const page of branches.values())
        for (const entry of page.entries) {
          if (
            entry.id === selected.id ||
            entry.relativePath.startsWith(selected.relativePath + "/")
          ) {
            branches.delete(entry.id);
            opened.delete(entry.id);
          }
        }
      this.update({ branches, opened });
      await this.refresh();
    } catch (e) {
      if (this.state.root?.rootId === root.rootId)
        this.update({ error: (e as Error).message });
    } finally {
      if (this.state.root?.rootId === root.rootId)
        this.update({ actionBusy: false });
    }
  }
  async select(entry: Entry, preserveError = false) {
    const root = this.state.root;
    if (!root) return;
    const rev = ++this.revision;
    this.send({ type: "unsubscribe" });
    this.bytes = new ByteWindow();
    this.update({
      selected: entry,
      text: "",
      start: "0",
      end: "0",
      loading: true,
      error: preserveError ? this.state.error : "",
      following: true,
      newer: false,
      evicted: false,
      status: "Chargement…",
    });
    try {
      const chunk = await this.api.request<Chunk>(
        `/api/v1/roots/${root.rootId}/files/${entry.id}/content`,
      );
      if (
        rev !== this.revision ||
        this.state.root?.rootId !== root.rootId ||
        this.stopped
      )
        return;
      this.bytes.replace(chunk);
      this.cursor = chunk.end;
      this.showBytes();
      this.update({
        status: this.state.connected ? "En direct" : "Reconnexion…",
      });
      this.subscribeFile();
    } catch (e) {
      if (rev === this.revision)
        this.update({
          error: (e as Error).message,
          status: "Lecture impossible",
        });
    } finally {
      if (rev === this.revision) this.update({ loading: false });
    }
  }
  setFollowing(value: boolean) {
    if (value && this.state.newer) return;
    this.update({ following: value });
  }
  async older() {
    const { root, selected } = this.state;
    if (!root || !selected || this.bytes.start === 0n || this.state.loading)
      return;
    const rev = this.revision;
    this.update({ loading: true, following: false, prependedRows: 0 });
    try {
      const c = await this.api.request<Chunk>(
        `/api/v1/roots/${root.rootId}/files/${selected.id}/content?before=${this.bytes.start}&generation=${this.bytes.generation}`,
      );
      if (rev !== this.revision) return;
      this.bytes.prepend(c);
      this.update({ prependedRows: atob(c.data).split("\n").length - 1 });
      this.showBytes();
      this.update({ newer: this.bytes.end < BigInt(this.cursor) });
    } catch (e) {
      if (rev === this.revision) {
        if (e instanceof ApiError && e.code === "GENERATION_CHANGED")
          await this.select(selected, true);
        else this.update({ error: (e as Error).message });
      }
    } finally {
      if (rev === this.revision) this.update({ loading: false });
    }
  }
  async bottom() {
    if (this.state.selected) await this.select(this.state.selected);
  }
  stop() {
    if (this.inventoryTimer) clearTimeout(this.inventoryTimer);
    this.stopped = true;
    this.revision++;
    this.rootRevision++;
    clearTimeout(this.retry);
    clearInterval(this.heartbeat);
    this.socket?.close();
    void this.api.close();
  }
}
