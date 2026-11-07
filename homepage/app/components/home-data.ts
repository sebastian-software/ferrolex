import { family } from "ferramenta-family/registry";

import homeSamples from "../data/home-samples.json";

/* Everything the homepage shows is generated, never edited by hand: the
   specimens in app/data/home-samples.json (scripts/render-home-samples.mjs,
   real output of the released crates), the locale status in
   app/data/compatibility.json (scripts/write-site-data.mjs, from the README's
   generated compat-status block, at build time). */
export type Suggestion = { word: string; distance: number };
export type Rule = { kind: string; strip: string; add: string };
export type Explained = {
  word: string;
  accepted: boolean;
  casing?: { path: string; candidate?: string };
  kind?: { type: string; stem?: string; rules?: Rule[] };
  reason?: { type: string; stem?: string };
};
export type Checked = {
  word: string;
  correct: boolean;
  suggestions?: Suggestion[];
  completeness?: string;
};
export type Limits = {
  maxResults: number;
  maxEditDistance: number;
  maxCandidates: number;
  maxEditCells: number;
};
export type BudgetRun = {
  id: string;
  completeness: string;
  suggestions: Suggestion[];
  limits: Limits;
};
type HomeSamples = {
  renderedWith: { ferrolex: string; tag: string; commit: string };
  catalog: { locales: number; revision: string };
  dictionaries: Array<{
    locale: string;
    license: string;
    aff: { name: string };
    dic: { name: string };
    stems: number;
  }>;
  explain: Array<{ locale: string; words: Explained[] }>;
  check: Array<{ locale: string; words: Checked[] }>;
  budget: { locale: string; word: string; runs: BudgetRun[] };
};

/** The generated file, read through the shape the specimens rely on. */
export const samples: HomeSamples = homeSamples;
export { default as compatibility } from "../data/compatibility.json";

/** The release that rendered the specimens. */
export const renderedWith = samples.renderedWith.ferrolex;
/** The upstream LibreOffice commit every catalog source is pinned to. */
export const catalogRevision = samples.catalog.revision.slice(0, 7);

const member = family.find((entry) => entry.name === "ferrolex");
if (!member) throw new Error("Ferrolex is missing from the family registry.");
export const ferrolex = member;

/** A registry note by its title, for the margin. */
export function registryNote(title: string) {
  const note = ferrolex.editorial?.notes.find((entry) => entry.title === title);
  if (!note) throw new Error(`The registry has no Ferrolex note "${title}".`);
  return note;
}

type Locale = "de_DE" | "en_US";

/** The locale's sample, by the order the samples program wrote them. */
export function byLocale<T extends { locale: string }>(list: T[], locale: Locale): T {
  const found = list.find((entry) => entry.locale === locale);
  if (!found) throw new Error(`The homepage samples have no ${locale} entry.`);
  return found;
}

/** The dictionary a locale's specimens were checked against. */
export const dictionary = (locale: Locale) => byLocale(samples.dictionaries, locale);

export const LANGUAGES: Record<Locale, string> = {
  en_US: "English",
  de_DE: "German",
};

const NUMBERS = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
/** Small counts in words, as the reading text writes them. */
export const spelled = (count: number) => NUMBERS[count] ?? count.toLocaleString("en-US");

/** The first letter in lower case, to set a registry phrase inside a sentence. */
export const lowerFirst = (text: string) => `${text.charAt(0).toLowerCase()}${text.slice(1)}`;
/** The first letter in upper case, for a label that starts with a count. */
export const capitalized = (text: string) => `${text.charAt(0).toUpperCase()}${text.slice(1)}`;
