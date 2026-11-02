/* eslint-disable security/detect-non-literal-fs-filename -- Every page comes from the repository-owned route manifest. */
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

import { expectedPages } from "./site-pages.mjs";

const repository = fileURLToPath(new URL("../../", import.meta.url));
const output = new URL("../build/client/", import.meta.url);
for (const page of expectedPages) {
  const source = await readFile(new URL(page, output), "utf8");
  const html = source.replaceAll(/<!--[\s\S]*?-->/g, "");
  assert.equal([...html.matchAll(/<h1\b/g)].length, 1, `${page} needs exactly one h1`);
  assert.equal(
    [...html.matchAll(/<main\b|role="main"/g)].length,
    1,
    `${page} needs exactly one main landmark`,
  );
  assert(
    html.includes('class="ferra-header ferra-wrap"') &&
      html.includes('class="ferra-footer ferra-wrap"'),
    `${page} is missing the shared shell`,
  );
  assert(!html.includes("site.css"), `${page} uses project-specific CSS`);
  if (page === "index.html") continue;
  const blocks = [...html.matchAll(/<pre\b[^>]*>/g)];
  assert(
    blocks.every(([tag]) => tag.includes('tabindex="0"')),
    `${page} has a code block without keyboard scrolling`,
  );
  const tables = [...html.matchAll(/<table\b/g)];
  const regions = [
    ...html.matchAll(/<div\s[^>]*role="region"[^>]*tabindex="0"[^>]*>[\s\S]*?<table\b/g),
  ];
  assert.equal(
    regions.length,
    tables.length,
    `${page} has a table without a keyboard-scrollable region`,
  );
}
// Run the native checker for pages, fragments, assets and the project base.
execFileSync("ferrolink", ["check", fileURLToPath(output), "--site-url", "https://ferrolex.dev/"], {
  cwd: repository,
  stdio: "inherit",
});
console.log(
  `Verified the shared shell and reading semantics on all ${expectedPages.length} pages.`,
);
