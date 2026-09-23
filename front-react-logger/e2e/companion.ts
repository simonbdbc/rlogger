/** Frontend test harness: launches the Rust binary; implements no backend logic. */
import { spawn } from "node:child_process";
import { once } from "node:events";
import path from "node:path";
import { fileURLToPath } from "node:url";
export async function startService(
  options: { port?: number; pollMs?: number } = {},
) {
  const root = path.resolve(
    path.dirname(fileURLToPath(import.meta.url)),
    "../..",
  );
  const process = spawn(
    path.join(root, "local-logs-server/target/debug/local-logs-server"),
    [
      "--port",
      String(options.port ?? 0),
      "--poll-ms",
      String(options.pollMs ?? 25),
      "--dist",
      path.join(root, "front-react-logger/dist"),
    ],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  let output = "",
    errors = "";
  process.stderr.on("data", (chunk) => {
    errors += chunk.toString();
  });
  const origin = await new Promise<string>((resolve, reject) => {
    const timer = setTimeout(() => {
      process.kill("SIGTERM");
      reject(Error("Rust companion startup timeout: " + errors));
    }, 10000);
    process.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    process.once("exit", (code) => {
      clearTimeout(timer);
      reject(Error(`Rust companion exited ${code}: ${errors}`));
    });
    process.stdout.on("data", (chunk) => {
      output += chunk.toString();
      const found = output.match(/http:\/\/127\.0\.0\.1:\d+/);
      if (found) {
        clearTimeout(timer);
        resolve(found[0]);
      }
    });
  });
  return {
    origin,
    async close() {
      if (process.exitCode !== null || process.signalCode !== null) return;
      const exit = once(process, "exit");
      process.kill("SIGTERM");
      await exit;
    },
  };
}
