/* eslint-disable security/detect-non-literal-fs-filename -- Source and output paths come from the repository-owned document manifest. */
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { posix } from "node:path";
import { format } from "oxfmt";

import { documents } from "./site-pages.mjs";

const repository = new URL("../../", import.meta.url);
const routes = new URL("../app/routes/", import.meta.url);
const data = new URL("../app/data/", import.meta.url);
const destinations = new Map(documents.map(({ source, path }) => [source, `/${path}/`]));
destinations.set("README.md", "/reference/overview/");
const repoUrl = "https://github.com/sebastian-software/ferrolex";
await mkdir(data, { recursive: true });
for (const section of ["guide", "reference", "evidence"]) {
  await rm(new URL(`${section}/`, routes), { recursive: true, force: true });
}

for (const document of documents) {
  const source = await readFile(new URL(document.source, repository), "utf8");
  const body = source.startsWith("---\n") ? source.slice(source.indexOf("\n---\n") + 5) : source;
  const content = rewriteLinks(body.replace(/^# .+\n/, ""), document.source);
  const destination = new URL(`${document.path}.md`, routes);
  await mkdir(new URL(".", destination), { recursive: true });
  await writeFile(destination, `---\ntitle: ${JSON.stringify(document.title)}\n---\n${content}`);
}

const cargo = await readFile(new URL("Cargo.toml", repository), "utf8");
const version = /^version = "([^"]+)"/m.exec(cargo)?.[1];
if (!version) throw new Error("The package has no release version.");
const workspacePackage = cargo.split(/^\[workspace\.package\]$/m)[1]?.split(/^\[/m)[0] ?? "";
const rustVersion = /^rust-version = "([^"]+)"/m.exec(workspacePackage)?.[1];
if (!rustVersion) throw new Error("The workspace has no rust-version.");
await writeData("site.json", { version, rustVersion });
await writeData("compatibility.json", await compatibility());
const labels = {
  guide: "Guide",
  reference: "Reference",
  evidence: "Evidence",
  changelog: "Changelog",
};
await writeData(
  "navigation.json",
  Object.entries(labels).map(([id, label]) => {
    const pages = documents
      .filter((document) => document.section === id)
      .map(({ path, title }) => ({ title, to: `/${path}/` }));
    return { id, label, to: pages[0].to, pages };
  }),
);
/**
 * The reviewed locale status from the README block that
 * scripts/generate-readme-compat-status.sh renders from the fixture catalog,
 * read at build time so the homepage never trails the README.
 */
async function compatibility() {
  const readme = await readFile(new URL("README.md.src", repository), "utf8");
  const block = /<!-- compat-status:start -->\n([\s\S]*?)<!-- compat-status:end -->/.exec(
    readme,
  )?.[1];
  if (!block) throw new Error("README.md.src has no compat-status block.");
  const statuses = { "✅": "ready", "🟡": "progress", "🔴": "blocked" };
  // Names are set here, at build time, so the server and the browser agree.
  const languageNames = new Intl.DisplayNames(["en-US"], {
    type: "language",
    languageDisplay: "standard",
  });
  const locales = block
    .split("\n")
    .filter((line) => line.startsWith("| `"))
    .map((line) => {
      const [locale, status] = line
        .split("|")
        .slice(1, 3)
        .map((cell) => cell.trim());
      const symbol = [...status][0];
      const state = statuses[symbol];
      if (!state) throw new Error(`Unknown compatibility status: ${status}`);
      const code = locale.replaceAll("`", "");
      return {
        locale: code,
        name: languageNames.of(code.replace("_", "-")),
        state,
        status: status.slice(symbol.length).trim(),
      };
    });
  if (locales.length === 0) throw new Error("The compat-status block lists no locales.");
  return { locales };
}

async function writeData(name, value) {
  const result = await format(name, JSON.stringify(value));
  if (result.errors.length) throw new Error(`Cannot format ${name}`);
  await writeFile(new URL(name, data), result.code);
}

function rewriteLinks(markdown, source) {
  let fence;
  return markdown
    .split("\n")
    .map((line) => {
      const delimiter = /^\s*(`{3,}|~{3,})/.exec(line);
      if (delimiter) {
        if (!fence)
          fence = {
            character: delimiter[1][0],
            length: delimiter[1].length,
            rust: line.slice(delimiter[0].length).trim() === "rust",
          };
        else if (delimiter[1][0] === fence.character && delimiter[1].length >= fence.length)
          fence = undefined;
        return line;
      }
      if (fence) return fence.rust && /^#(?: |$)/.test(line) ? null : line;
      return line.replaceAll(/(?<=\]\()[^\s)]+/g, (href) => resolveLink(href, source));
    })
    .filter((line) => line !== null)
    .join("\n");
}

function resolveLink(href, source) {
  if (/^(?:[a-z][a-z0-9+.-]*:|\/|#)/i.test(href)) return href;
  const [file, fragment] = href.split("#");
  const target = posix.normalize(posix.join(posix.dirname(source), file));
  const destination = destinations.get(target);
  if (destination) {
    return `${destination}${fragment ? `#${fragment}` : ""}`;
  }
  const kind = file.endsWith("/") || !posix.extname(file) ? "tree" : "blob";
  return `${repoUrl}/${kind}/main/${target}${fragment ? `#${fragment}` : ""}`;
}
