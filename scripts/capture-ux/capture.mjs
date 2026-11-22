// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Capture dashboard screenshots into docs/ux from a running Paqtra.
//
//   cd scripts/capture-ux && npm install
//   PAQTRA_URL=https://<host>:<port> PAQTRA_CDP=http://127.0.0.1:9222 npm run capture
//   PAQTRA_URL=... PAQTRA_USER=admin PAQTRA_PASSWORD=... npm run capture
//
// Auth, pick one:
//   PAQTRA_CDP                      attach to a Chrome you already signed in to
//                                   (start it with --remote-debugging-port=9222)
//   PAQTRA_USER + PAQTRA_PASSWORD   sign in with a fresh headless Chrome
// Credentials are read from the environment only and are never written anywhere.
import puppeteer from "puppeteer-core";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";

const URL_BASE = (process.env.PAQTRA_URL || "").replace(/\/+$/, "");
const CDP = process.env.PAQTRA_CDP;
const USER = process.env.PAQTRA_USER;
const PASS = process.env.PAQTRA_PASSWORD;
const CHROME =
  process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const OUT =
  process.env.OUT ||
  path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../docs/ux");

if (!URL_BASE) fail("set PAQTRA_URL (e.g. https://host:30813)");
if (!CDP && !(USER && PASS)) fail("set PAQTRA_CDP, or PAQTRA_USER and PAQTRA_PASSWORD");

// [file name, route, extra settle ms]. Routes come from web-ui/src/navConfig.ts.
const SHOTS = [
  ["00-overview", "/", 2500],
  ["01-flows", "/flows", 3000],
  ["02-investigate", "/investigate", 2000],
  ["03-drops", "/drops", 2500],
  ["04-service-map", "/servicemap", 3000],
  ["05-topology", "/topology", 3000],
  ["06-policies", "/policies", 2500],
  ["07-ebpf", "/ebpf", 3000],
  ["08-diagnostics", "/diagnostics", 2500],
];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
function fail(msg) {
  console.error(`capture-ux: ${msg}`);
  process.exit(1);
}

fs.mkdirSync(OUT, { recursive: true });

const browser = CDP
  ? await puppeteer.connect({ browserURL: CDP, defaultViewport: { width: 1440, height: 900 } })
  : await puppeteer.launch({
      headless: true,
      executablePath: CHROME,
      ignoreHTTPSErrors: true,
      args: ["--ignore-certificate-errors", "--window-size=1440,900"],
      defaultViewport: { width: 1440, height: 900 },
    });

const page = await browser.newPage();
page.setDefaultTimeout(45000);
await page.setViewport({ width: 1440, height: 900 });

// `load` (not networkidle2): the dashboard keeps WebSockets open, so
// networkidle never settles against a live cluster.
async function gotoSettle(url, settleMs = 1500) {
  await page.goto(url, { waitUntil: "load", timeout: 60000 });
  await sleep(settleMs);
}

try {
  await gotoSettle(URL_BASE + "/", 2000);

  if (!CDP) {
    await page.screenshot({ path: path.join(OUT, "login.png") });
    await page.waitForSelector("input[type=password]");
    const userSel = "input[name=username], input[autocomplete=username], input[type=text]";
    await page.click(userSel, { clickCount: 3 });
    await page.type(userSel, USER, { delay: 15 });
    await page.type("input[type=password]", PASS, { delay: 15 });
    await page.click("button[type=submit], form button");
    await page.waitForFunction(() => !document.querySelector("input[type=password]"), {
      timeout: 30000,
    });
  } else if (await page.$("input[type=password]")) {
    fail("the attached Chrome is not signed in to PAQTRA_URL");
  }

  for (const [name, route, settle] of SHOTS) {
    await gotoSettle(URL_BASE + route, settle);
    // Empty-state tiles render "—"; warn so a run against an idle cluster is not committed unnoticed.
    const empty = await page.evaluate(
      () => document.body.innerText.split("\n").filter((l) => l.trim() === "—").length
    );
    if (empty > 2) console.warn(`  ${name}: ${empty} empty "—" tiles, page may not have data yet`);
    await page.screenshot({ path: path.join(OUT, `${name}.png`) });
    console.log(`wrote ${path.join(OUT, name)}.png`);
  }
} finally {
  await page.close();
  if (CDP) browser.disconnect();
  else await browser.close();
}
