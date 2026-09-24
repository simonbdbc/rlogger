export class ApiError extends Error {
  constructor(
    public code: string,
    message: string,
  ) {
    super(message);
  }
}
export class Api {
  token = "";
  serverId = "";
  private queue: Promise<unknown> = Promise.resolve();
  private waiting = 0;
  async initialize() {
    const response = await fetch("/api/v1/session", { method: "POST" });
    const data = await response.json();
    if (!response.ok) throw new ApiError(data.code, data.message);
    this.token = data.token;
    this.serverId = data.serverId;
  }
  request<T>(url: string, options: RequestInit = {}): Promise<T> {
    if (this.waiting >= 16)
      return Promise.reject(
        new ApiError("LIMIT", "Trop de requêtes en attente."),
      );
    this.waiting++;
    const next = this.queue
      .catch(() => {})
      .then(async () => {
        for (let attempt = 0; attempt < 20; attempt++) {
          const response = await fetch(url, {
            ...options,
            headers: {
              "Content-Type": "application/json",
              "x-local-session": this.token,
              ...options.headers,
            },
          });
          const data = await response.json();
          if (response.status === 429 && attempt < 19) {
            await new Promise((resolve) => setTimeout(resolve, 50));
            continue;
          }
          if (!response.ok) throw new ApiError(data.code, data.message);
          return data as T;
        }
        throw Error("Lecture indisponible");
      })
      .finally(() => {
        this.waiting--;
      });
    this.queue = next;
    return next;
  }
  async close() {
    if (this.token)
      await fetch("/api/v1/session", {
        method: "DELETE",
        headers: { "x-local-session": this.token },
        keepalive: true,
      }).catch(() => {});
  }
}
