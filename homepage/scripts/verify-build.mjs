/* eslint-disable security/detect-non-literal-fs-filename -- Every page comes from the repository-owned route manifest. */
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readdir, readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

import { expectedPages } from "./site-pages.mjs";

const data = (name) => new URL(`../app/data/${name}`, import.meta.url);
const site = JSON.parse(await readFile(data("site.json"), "utf8"));
const samples = JSON.parse(await readFile(data("home-samples.json"), "utf8"));
const compatibility = JSON.parse(await readFile(data("compatibility.json"), "utf8"));
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
  for (const fragment of [
    'class="mono-header"',
    'class="mono-footer"',
    `<span class="mono-release" title="Current release">v${site.version}</span>`,
    "https://ferramenta.dev",
  ]) {
    assert(html.includes(fragment), `${page} is missing ${fragment} from the shared shell`);
  }
  assert(!html.includes("site.css"), `${page} uses project-specific CSS`);
  if (page === "index.html") {
    verifyHomepage(html);
    continue;
  }
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
await verifyScopedCss();

/** The homepage reads on the Monografie page, keeps its anchors and shows the data's figures. */
function verifyHomepage(html) {
  assert(html.includes('class="mono flx-home"'), "The homepage should use the Monografie page.");
  assert(
    html.includes('data-product="ferrolex"'),
    "The homepage opening should show Ferrolex's own instrument.",
  );
  for (const id of ["overview", "example", "limits", "compatibility", "start"]) {
    assert(html.includes(`id="${id}"`), `Preserve the homepage anchor #${id}.`);
  }
  const text = html.replaceAll(/<[^<>]*>/g, " ").replaceAll(/\s+/g, " ");
  const ready = compatibility.locales.filter((entry) => entry.state === "ready").length;
  for (const claim of [
    `Ferrolex ${site.version.split(".").slice(0, 2).join(".")},`,
    ` ${samples.catalog.locales} LibreOffice dictionaries in the reviewed catalog`,
    ` ${ready} of ${compatibility.locales.length} tested locales`,
    `Rust ${site.rustVersion}+`,
    `ferrolex-hunspell ${samples.renderedWith.ferrolex}`,
  ]) {
    assert(text.includes(claim), `The homepage no longer states "${claim.trim()}" from its data.`);
  }
  assert(!/NaN|undefined|Infinity/.test(text), "The homepage renders a missing figure.");
}

/** A selector list split at its top-level commas, so `:is(a, b)` stays whole. */
function selectorList(prelude) {
  const selectors = [""];
  let depth = 0;
  for (const char of prelude) {
    if (char === "(") depth += 1;
    if (char === ")") depth -= 1;
    if (char === "," && depth === 0) selectors.push("");
    else selectors[selectors.length - 1] += char;
  }
  return selectors;
}

/**
 * Shared presentation lives in ferramenta-family. The site styles only its own
 * homepage specimens and shell seams, next to their components, under flx-.
 */
async function verifyScopedCss() {
  const appFiles = await readdir(new URL("../app/", import.meta.url), { recursive: true });
  for (const file of appFiles.filter((name) => name.endsWith(".css"))) {
    assert(
      file.startsWith("components/"),
      `Keep ${file} beside its specimen in app/components/, or in ferramenta-family.`,
    );
    const css = await readFile(new URL(`../app/${file}`, import.meta.url), "utf8");
    const preludes = css
      .replaceAll(/\/\*[\s\S]*?\*\//g, "")
      .split("{")
      .slice(0, -1)
      .map((chunk) =>
        chunk.slice(Math.max(chunk.lastIndexOf("}"), chunk.lastIndexOf(";")) + 1).trim(),
      )
      .filter((prelude) => !prelude.startsWith("@"));
    for (const selector of preludes.flatMap((prelude) => selectorList(prelude))) {
      assert(selector.includes(".flx-"), `${file}: scope "${selector.trim()}" to an flx- class.`);
    }
  }
}

// Run the native checker for pages, fragments, assets and the project base.
execFileSync("ferrolink", ["check", fileURLToPath(output), "--site-url", "https://ferrolex.dev/"], {
  cwd: repository,
  stdio: "inherit",
});
console.log(
  `Verified the shared shell and reading semantics on all ${expectedPages.length} pages.`,
);
