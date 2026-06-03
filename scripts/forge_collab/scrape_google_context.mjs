#!/usr/bin/env node
/**
 * Scrape the user's Google AI/search URL into var/forge_collab/scrapes/
 * Usage: node scripts/forge_collab/scrape_google_context.mjs [url]
 */
import { chromium } from "playwright";
import { writeFileSync, mkdirSync } from "fs";
import { dirname, join } from "path";
import { fileURLToPath } from "url";

const DEFAULT_URL =
  "https://www.google.com/search?q=straights+of+mackinac+bag+file+survey+numbers&hl=en-US&udm=50";

const url = process.argv[2] || process.env.COLLAB_GOOGLE_URL || DEFAULT_URL;
const repo = join(dirname(fileURLToPath(import.meta.url)), "../..");
const outDir = join(repo, "var/forge_collab/scrapes");
mkdirSync(outDir, { recursive: true });

const headless = process.env.HEADLESS !== "0";
const browser = await chromium.launch({ headless });
const page = await browser.newPage();
await page.setViewportSize({ width: 1400, height: 900 });

try {
  await page.goto(url, { waitUntil: "networkidle", timeout: 120000 });
  await page.waitForTimeout(5000);
  const text = await page.innerText("body");
  const html = await page.content();
  const shot = join(outDir, "google_straits_bag_survey.png");
  await page.screenshot({ path: shot, fullPage: true });
  writeFileSync(join(outDir, "google_straits_bag_survey.txt"), text.slice(0, 200000));
  writeFileSync(join(outDir, "google_straits_bag_survey.meta.json"), JSON.stringify({
    url,
    scraped_at: new Date().toISOString(),
    text_chars: text.length,
    screenshot: shot,
  }, null, 2));
  console.log(`OK text=${text.length} -> ${outDir}`);
} catch (e) {
  writeFileSync(join(outDir, "google_straits_bag_survey.error.txt"), String(e));
  console.error(e);
  process.exitCode = 1;
} finally {
  await browser.close();
}
