import { test, expect, type Page } from "@playwright/test";
import {
  mkdtemp,
  writeFile,
  appendFile,
  rm,
  mkdir,
  rename,
  readFile,
  readdir,
  access,
} from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { startService } from "./companion";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
let service: Awaited<ReturnType<typeof startService>>;
let dir: string;
test.beforeEach(async ({ page }) => {
  service = await startService({ port: 0, pollMs: 25 });
  dir = await mkdtemp(path.join(os.tmpdir(), "local-logs-e2e-"));
  await writeFile(path.join(dir, "a.log"), "A initial 🦀\n");
  await writeFile(path.join(dir, "b.txt"), "B initial\n");
  await writeFile(path.join(dir, "empty.log"), "");
  await page.goto(service.origin);
  await expect(
    page.getByRole("heading", { name: "Local Logs", exact: true }),
  ).toBeVisible();
});
test.afterEach(async ({ page }) => {
  await page.close();
  await service.close();
  await rm(dir, { recursive: true, force: true });
});
async function open(page: Page, directory = dir) {
  await page.getByRole("textbox", { name: "Dossier local" }).fill(directory);
  await page.getByRole("textbox", { name: "Dossier local" }).press("Enter");
  await expect(
    page.getByRole("button", { name: "Ouvrir", exact: true }),
  ).toBeEnabled();
  await expect(
    page.getByRole("treeitem", { name: "a.log", exact: true }),
  ).toBeVisible();
}
const selectedAction = (page: Page, action: string) =>
  page
    .getByRole("treeitem", { selected: true })
    .locator("..")
    .getByRole("button", { name: new RegExp("^" + action + " le fichier ") });
const content = (page: Page) =>
  page.getByLabel("Contenu du fichier", { exact: true });
test("source menu keeps separate paths and omits maintenance warning for external logs", async ({
  page,
}) => {
  const rlogger = page.getByRole("button", { name: /Journaux RLOGGER/ });
  const external = page.getByRole("button", { name: /Journaux externes/ });
  await expect(rlogger).toHaveAttribute("aria-current", "page");
  await open(page);
  await expect(
    page.getByText("Maintenance automatique RLOGGER indisponible.", {
      exact: false,
    }),
  ).toBeVisible();
  await external.click();
  await expect(external).toHaveAttribute("aria-current", "page");
  await expect(
    page.getByRole("treeitem", { name: "a.log", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByText("Maintenance automatique RLOGGER indisponible.", {
      exact: false,
    }),
  ).toHaveCount(0);
  const request = page.waitForRequest(
    (candidate) =>
      candidate.url().endsWith("/api/v1/roots") &&
      candidate.method() === "POST" &&
      candidate.postDataJSON()?.maintenance === false,
  );
  await open(page);
  await request;
  await expect(
    page.getByText("Maintenance automatique RLOGGER indisponible.", {
      exact: false,
    }),
  ).toHaveCount(0);
  await rlogger.click();
  await expect(
    page.getByRole("textbox", { name: "Dossier local" }),
  ).toHaveValue(dir);
  await external.click();
  await expect(
    page.getByRole("textbox", { name: "Dossier local" }),
  ).toHaveValue(dir);
});
test("path, empty state, real append, error and raw HTML remain distinct", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await open(page);
  await page.getByRole("treeitem", { name: "empty.log", exact: true }).click();
  await expect(content(page)).toContainText("Ce fichier est vide");
  await appendFile(
    path.join(dir, "empty.log"),
    "<script>window.bad=true</script> 🦀\r\n",
  );
  await expect(content(page)).toContainText("<script>window.bad=true</script>");
  expect(await page.evaluate(() => "bad" in window)).toBe(false);
  await page
    .getByRole("textbox", { name: "Dossier local" })
    .fill("/missing-synthetic-root");
  await page.getByRole("textbox", { name: "Dossier local" }).press("Enter");
  await expect(page.getByRole("alert")).toContainText("disparu");
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await expect(content(page)).toContainText("A initial");
  expect(errors).toEqual([]);
});
test("rapid A/B/A and a changed root never mix content", async ({ page }) => {
  await open(page);
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await page.getByRole("treeitem", { name: "b.txt", exact: true }).click();
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await appendFile(path.join(dir, "b.txt"), "B must stay elsewhere");
  await appendFile(path.join(dir, "a.log"), "A live");
  await expect(content(page)).toContainText("A live");
  await expect(content(page)).not.toContainText("B initial");
  await mkdir(path.join(dir, "new"));
  await writeFile(path.join(dir, "new", "a.log"), "NEW ROOT");
  await open(page, path.join(dir, "new"));
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await expect(content(page)).toHaveText("NEW ROOT");
});
test("historical reading stays stable during bursts and reaches the beginning", async ({
  page,
}) => {
  await writeFile(
    path.join(dir, "a.log"),
    Array.from(
      { length: 40000 },
      (_, i) => `line-${i.toString().padStart(6, "0")} ${"x".repeat(30)}\n`,
    ).join(""),
  );
  await open(page);
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await expect(content(page)).toContainText("line-039999");
  await content(page).evaluate((el) => {
    el.scrollTop = 0;
    el.dispatchEvent(new Event("scroll", { bubbles: true }));
  });
  await expect(content(page)).not.toContainText("line-039999");
  const before = await content(page).textContent();
  await appendFile(path.join(dir, "a.log"), "BURST-MARKER\n".repeat(200));
  await expect(page.getByText("Nouveaux octets disponibles")).toBeVisible();
  expect(await content(page).textContent()).toBe(before);
  for (let i = 0; i < 10; i++) {
    const button = page.getByRole("button", {
      name: "Charger plus ancien",
      exact: true,
    });
    if (await button.isDisabled()) break;
    await button.click();
    await expect(
      page.getByRole("button", { name: "Ouvrir", exact: true }),
    ).toBeEnabled();
    await content(page).evaluate((el) => {
      el.scrollTop = 0;
      el.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
  }
  await expect(content(page)).toContainText("line-000000");
  await page
    .getByRole("button", { name: "↓ Retour en bas", exact: true })
    .click();
  await expect(content(page)).toContainText("BURST-MARKER");
});
test("truncate, replace and restart revalidate without concatenating generations", async ({
  page,
}) => {
  await open(page);
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await expect(content(page)).toContainText("A initial");
  await writeFile(path.join(dir, "a.log"), "T\n");
  await expect(content(page)).toHaveText("T\n");
  await rename(path.join(dir, "a.log"), path.join(dir, "old.log"));
  await writeFile(path.join(dir, "a.log"), "REPLACEMENT\n");
  await expect(content(page)).toHaveText("REPLACEMENT\n");
  const port = Number(new URL(service.origin).port);
  await service.close();
  service = await startService({ port, pollMs: 25 });
  await expect(
    page.getByRole("treeitem", { name: "a.log", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Lecteur de logs", exact: true }),
  ).toBeVisible();
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await expect(content(page)).toHaveText("REPLACEMENT\n");
});
test("two clients of one file remain independent and new files appear", async ({
  page,
  context,
}) => {
  await open(page);
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  const second = await context.newPage();
  await second.goto(service.origin);
  await open(second);
  await second.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await appendFile(path.join(dir, "a.log"), "BOTH CLIENTS\n");
  await expect(content(page)).toContainText("BOTH CLIENTS");
  await expect(content(second)).toContainText("BOTH CLIENTS");
  await second.close();
  await appendFile(path.join(dir, "a.log"), "FIRST STILL LIVE\n");
  await expect(content(page)).toContainText("FIRST STILL LIVE");
  await writeFile(path.join(dir, "created.log"), "new");
  await expect(
    page.getByRole("treeitem", { name: "created.log", exact: true }),
  ).toBeVisible();
});
test("expired session is renewed on the same companion", async ({ page }) => {
  await open(page);
  const request = page.waitForRequest((request) =>
    request.url().includes("/content"),
  );
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await expect(content(page)).toContainText("A initial");
  const token = await (await request).headerValue("x-local-session");
  const closed = await fetch(service.origin + "/api/v1/session", {
    method: "DELETE",
    headers: { Origin: service.origin, "x-local-session": token! },
  });
  expect(closed.status).toBe(200);
  await expect(
    page.getByRole("heading", { name: "Lecteur de logs", exact: true }),
  ).toBeVisible();
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await appendFile(path.join(dir, "a.log"), "AFTER SESSION RENEWAL\n");
  await expect(content(page)).toContainText("AFTER SESSION RENEWAL");
});

async function managed(page: Page) {
  await promisify(execFile)(
    path.resolve("../local-logs-server/target/debug/examples/fixtures"),
    [dir, "--managed"],
  );
  const root = path.join(dir, "rlogger");
  await page.getByRole("textbox", { name: "Dossier local" }).fill(root);
  await page.getByRole("textbox", { name: "Dossier local" }).press("Enter");
  await page.getByRole("treeitem", { name: "1970-01-01", exact: true }).click();
  await page.getByRole("treeitem", { name: "test", exact: true }).click();
  return root;
}
test("managed sizes, native download and confirmed deletion preserve cancellation", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  const root = await managed(page);
  const name = "archive-00-1-2-3-h0-s1.log";
  await page.getByRole("treeitem", { name, exact: true }).click();
  await expect(content(page)).toContainText("Archive téléchargeable");
  await expect(page.locator(".root-sizes")).toContainText("Alloué");
  const downloading = page.waitForEvent("download");
  await selectedAction(page, "Télécharger").click();
  const download = await downloading;
  expect(download.suggestedFilename()).toBe(name);
  expect(await readFile((await download.path())!, "utf8")).toBe(
    "Archive téléchargeable 🦀\n",
  );
  await selectedAction(page, "Supprimer").click();
  await page.getByRole("button", { name: "Annuler", exact: true }).click();
  await access(path.join(root, "1970-01-01/test", name));
  await selectedAction(page, "Supprimer").click();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("alertdialog")).toHaveCount(0);
  await expect(selectedAction(page, "Supprimer")).toBeFocused();
  await selectedAction(page, "Supprimer").click();
  await expect(page.getByRole("alertdialog")).toContainText(
    `1970-01-01/test/${name}`,
  );
  await page.screenshot({
    path: path.join(dir, `rlogger-confirm-${test.info().project.name}.png`),
  });
  await page
    .getByRole("button", { name: "Confirmer la suppression", exact: true })
    .click();
  await expect(page.getByRole("treeitem", { name, exact: true })).toHaveCount(
    0,
  );
  await expect(
    access(path.join(root, "1970-01-01/test", name)),
  ).rejects.toThrow();
  expect(errors).toEqual([]);
  await page.screenshot({
    path: path.join(dir, `rlogger-managed-${test.info().project.name}.png`),
  });
});
test("automatic recovery and empty day cleanup keep recovered bytes", async ({
  page,
}) => {
  const root = await managed(page);
  const recovered = "crash-00-1-2-3-h0-s2.recovered.log";
  await page.getByRole("treeitem", { name: recovered, exact: true }).click();
  await expect(content(page)).toHaveText("Dernière ligne incomplète");
  await expect(page.locator(".file-notice")).toContainText(
    "Récupéré après interruption",
  );
  await expect(selectedAction(page, "Télécharger")).toBeEnabled();
  await expect
    .poll(
      async () =>
        await access(path.join(root, "1970-01-02")).then(
          () => true,
          () => false,
        ),
    )
    .toBe(false);
});
test("active publication preserves the selected content and allows current-hour actions", async ({
  page,
}) => {
  const root = await managed(page);
  const day = (await readdir(root)).find(
    (s) => s !== ".rlogger" && s !== "1970-01-01" && s !== "1970-01-02",
  )!;
  await page.getByRole("treeitem", { name: "1970-01-01", exact: true }).click();
  await page.getByRole("treeitem", { name: day, exact: true }).click();
  await page.getByRole("treeitem", { name: "test", exact: true }).click();
  const name = (await readdir(path.join(root, day, "test")))[0]!;
  await page.getByRole("treeitem", { name, exact: true }).click();
  await expect(content(page)).toHaveText("Fichier actif\n");
  await expect(selectedAction(page, "Télécharger")).toBeEnabled();
  const closed = name.replace(".active.log", ".log");
  await rename(
    path.join(root, day, "test", name),
    path.join(root, day, "test", closed),
  );
  await expect(
    page.getByRole("heading", { name: closed, exact: true }),
  ).toBeVisible();
  await expect(content(page)).toHaveText("Fichier actif\n");
  await expect(selectedAction(page, "Supprimer")).toBeEnabled();
});
test("generic files display sizes and allow download and deletion", async ({
  page,
}) => {
  await open(page);
  await expect(page.locator(".root-notice")).toContainText(
    "Téléchargement et suppression restent disponibles",
  );
  const remove = page.getByRole("button", {
    name: "Supprimer le fichier a.log",
    exact: true,
  });
  await expect(remove).toHaveText("");
  await expect(remove).toHaveAttribute("title", "Supprimer le fichier a.log");
  await remove.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("alertdialog")).toContainText("a.log");
  await page.keyboard.press("Escape");
  await expect(remove).toBeFocused();
  await expect(content(page)).toContainText("Choisissez un fichier");
  await page.getByRole("treeitem", { name: "a.log", exact: true }).click();
  await expect(
    page
      .locator(".reader")
      .getByRole("button", { name: /^(Télécharger|Supprimer)/ }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("treeitem", { name: "a.log", exact: true }),
  ).toContainText("Alloué");
  await expect(selectedAction(page, "Télécharger")).toBeEnabled();
  const downloading = page.waitForEvent("download");
  await selectedAction(page, "Télécharger").click();
  const download = await downloading;
  expect(await readFile((await download.path())!, "utf8")).toBe(
    "A initial 🦀\n",
  );
  const otherDownload = page.waitForEvent("download");
  await page
    .getByRole("button", { name: "Télécharger le fichier b.txt", exact: true })
    .click();
  expect(await readFile((await (await otherDownload).path())!, "utf8")).toBe(
    "B initial\n",
  );
  await expect(content(page)).toContainText("A initial");
  await selectedAction(page, "Supprimer").click();
  await page
    .getByRole("button", { name: "Confirmer la suppression", exact: true })
    .click();
  await expect(
    page.getByRole("treeitem", { name: "a.log", exact: true }),
  ).toHaveCount(0);
});
test("a changed file is kept when deletion precondition fails", async ({
  page,
}) => {
  const root = await managed(page);
  const name = "archive-00-1-2-3-h0-s1.log";
  await page.getByRole("treeitem", { name, exact: true }).click();
  await page.route("**/api/v1/roots/*/files/*", async (route) => {
    if (route.request().method() === "DELETE")
      await writeFile(
        path.join(root, "1970-01-01/test", name),
        "Changed externally before deletion\n",
      );
    await route.continue();
  });
  await selectedAction(page, "Supprimer").click();
  await page
    .getByRole("button", { name: "Confirmer la suppression", exact: true })
    .click();
  await expect(page.getByRole("alert")).toContainText("Le fichier a changé");
  await expect(page.getByRole("treeitem", { name, exact: true })).toBeVisible();
  expect(await readFile(path.join(root, "1970-01-01/test", name), "utf8")).toBe(
    "Changed externally before deletion\n",
  );
});

test("folder download and recursive deletion include hidden files and close the selected child", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await mkdir(path.join(dir, "bundle", "empty"), { recursive: true });
  await writeFile(path.join(dir, "bundle", ".hidden"), "hidden\n");
  await mkdir(path.join(dir, ".rlogger"));
  await mkdir(path.join(dir, "bundle", ".hidden-dir"));
  await writeFile(path.join(dir, "bundle", "data.json"), '{"test":true}\n');
  await open(page);
  const folder = page.getByRole("treeitem", { name: "bundle", exact: true });
  const removeFolder = page.getByRole("button", {
    name: "Supprimer le dossier bundle",
    exact: true,
  });
  await expect(removeFolder).toBeVisible();
  await expect(removeFolder).toHaveText("");
  await expect(removeFolder).toHaveAttribute(
    "title",
    "Supprimer le dossier bundle",
  );
  await removeFolder.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("alertdialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(removeFolder).toBeFocused();
  await expect(folder).toHaveAttribute("aria-expanded", "false");
  await page.getByRole("treeitem", { name: "bundle", exact: true }).click();
  await expect(
    page.getByRole("treeitem", { name: ".hidden", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("treeitem", { name: ".rlogger", exact: true }),
  ).toHaveCount(0);
  await expect(
    page.getByRole("treeitem", { name: ".hidden-dir", exact: true }),
  ).toHaveCount(0);
  await page.getByRole("treeitem", { name: "data.json", exact: true }).click();
  await expect(content(page)).toContainText('{"test":true}');
  const pending = page.waitForEvent("download");
  await page
    .getByRole("button", { name: "Télécharger le dossier bundle", exact: true })
    .click();
  const downloaded = await pending;
  expect(downloaded.suggestedFilename()).toBe("bundle.tar");
  const bytes = await readFile((await downloaded.path())!);
  expect(bytes.toString()).toContain("bundle/.hidden");
  expect(bytes.toString()).toContain("hidden\n");
  expect(bytes.toString()).toContain('{"test":true}\n');
  await page
    .getByRole("button", { name: "Supprimer le dossier bundle", exact: true })
    .click();
  await expect(page.getByRole("alertdialog")).toContainText(
    "Tout le contenu du dossier",
  );
  await page.keyboard.press("Escape");
  await access(path.join(dir, "bundle", ".hidden"));
  await page
    .getByRole("button", { name: "Supprimer le dossier bundle", exact: true })
    .click();
  await page.screenshot({
    path: `/private/tmp/rlogger-folder-${test.info().project.name}.png`,
  });
  await page
    .getByRole("button", { name: "Confirmer la suppression", exact: true })
    .click();
  await expect(
    page.getByRole("treeitem", { name: "bundle", exact: true }),
  ).toHaveCount(0);
  await expect(content(page)).toContainText("Choisissez un fichier");
  await expect(access(path.join(dir, "bundle"))).rejects.toThrow();
  expect(errors).toEqual([]);
});
