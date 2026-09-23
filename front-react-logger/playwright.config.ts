import { defineConfig } from "@playwright/test";
import { mkdtempSync } from "node:fs";
import os from "node:os";
import path from "node:path";
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  timeout: 30000,
  outputDir: mkdtempSync(path.join(os.tmpdir(), "local-logs-playwright-")),
  reporter: "list",
  use: { browserName: "webkit", headless: true },
  projects: [
    { name: "webkit-desktop", use: { viewport: { width: 1280, height: 800 } } },
    { name: "webkit-small", use: { viewport: { width: 390, height: 844 } } },
  ],
});
