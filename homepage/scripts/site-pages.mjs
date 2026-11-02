export const documents = [
  {
    source: "homepage/content/getting-started.md",
    path: "guide/getting-started",
    title: "Getting started",
    section: "guide",
  },
  {
    source: "README.md.src",
    path: "reference/overview",
    title: "Engine and CLI",
    section: "reference",
  },
  {
    source: "docs/dictionary-fetching.md",
    path: "guide/dictionaries",
    title: "Dictionary acquisition",
    section: "guide",
  },
  {
    source: "docs/locale-compatibility.md",
    path: "evidence/compatibility",
    title: "Compatibility boundaries",
    section: "evidence",
  },
  {
    source: "docs/adr/0009-language-aware-source-analysis.md",
    path: "evidence/product-boundary",
    title: "Why parsing stays outside",
    section: "evidence",
  },
  { source: "CHANGELOG.md", path: "changelog", title: "Changelog", section: "changelog" },
];
export const expectedPages = ["index.html", ...documents.map(({ path }) => `${path}/index.html`)];
export function pagePath(page) {
  return `/${page === "index.html" ? "" : page.replace(/index\.html$/, "")}`;
}
