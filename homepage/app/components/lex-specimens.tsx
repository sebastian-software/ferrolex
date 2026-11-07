import { type ReactNode, useState } from "ardo/react";
import {
  MonoCaption,
  MonoFacing,
  MonoSegmented,
  MonoSheet,
  MonoWide,
} from "ferramenta-family/monografie";

import {
  type BudgetRun,
  byLocale,
  capitalized,
  catalogRevision,
  type Checked,
  dictionary,
  type Explained,
  LANGUAGES,
  renderedWith,
  type Rule,
  samples,
  spelled,
  type Suggestion,
} from "./home-data";
import "./lex-specimens.css";

/* The homepage's specimens: every verdict, derivation and suggestion here is
   real output of the released crates, from app/data/home-samples.json. */

type Locale = keyof typeof LANGUAGES;
const LOCALES: Locale[] = ["en_US", "de_DE"];
const isLocale = (value: string): value is Locale => value === "en_US" || value === "de_DE";
const languageOptions = LOCALES.map((locale) => ({ value: locale, label: LANGUAGES[locale] }));

/** One affix step in words: what the rule removes from the stem and what it adds. */
function step(rule: Rule) {
  const side = rule.kind === "prefix" ? "at the start" : "at the end";
  if (rule.strip.length === 0) return `adds ${rule.add} ${side}`;
  return `removes ${rule.strip} and adds ${rule.add} ${side}`;
}

/** A term and its value in a derivation or a list of limits. */
function Fact({ term, children }: { term: ReactNode; children: ReactNode }) {
  return (
    <div>
      <dt>{term}</dt>
      <dd>{children}</dd>
    </div>
  );
}

/** The affix rules of a derivation, in words. */
function rulesText(rules: Rule[]) {
  if (rules.length === 0) return "None. The word is stored as written.";
  return rules.map((rule) => `The ${rule.kind} rule ${step(rule)}.`).join(" ");
}

/** How the dictionary arrived at a word, or why it found no way. */
function Derivation({ entry }: { entry: Explained }) {
  const { kind, casing } = entry;
  if (!entry.accepted || kind === undefined) {
    return (
      <dl className="flx-facts">
        <Fact term="Verdict">Not accepted</Fact>
        <Fact term="Why">No stored word, affix rule or compound accepts this spelling.</Fact>
      </dl>
    );
  }
  const fallback = casing?.candidate;
  return (
    <dl className="flx-facts">
      <Fact term="Verdict">Accepted</Fact>
      <Fact term="Stored word">
        <code>{kind.stem ?? entry.word}</code>
      </Fact>
      <Fact term="Rule">{rulesText(kind.rules ?? [])}</Fact>
      <Fact term="Capitals">
        {fallback === undefined ? (
          "Matched as written."
        ) : (
          <>
            Matched as <code>{fallback}</code>, the way Hunspell lets a capitalized word match its
            lowercase entry.
          </>
        )}
      </Fact>
    </dl>
  );
}

/** Which dictionary and release answered, and what this release cannot do yet. */
function ExplainCaption({ locale }: { locale: Locale }) {
  const source = dictionary(locale);
  return (
    <MonoCaption>
      Real output of ferrolex-hunspell {renderedWith}, with the LibreOffice {LANGUAGES[locale]}{" "}
      dictionary ({source.aff.name} and {source.dic.name}, {source.stems.toLocaleString("en-US")}{" "}
      stored words) at upstream commit {catalogRevision}. The explanation API is marked
      experimental.
      {locale === "de_DE" && (
        <>
          {" "}
          German writes compounds as one word. This release does not yet put them together from this
          dictionary’s parts, so it rejects Wörterbuch (dictionary).
        </>
      )}
    </MonoCaption>
  );
}

/** A word, explained against its dictionary: stem, affix rule and capitals. */
export function ExplainSpecimen() {
  const [locale, setLocale] = useState<Locale>("en_US");
  const words = byLocale(samples.explain, locale).words;
  const [chosen, setChosen] = useState<Record<Locale, string>>({
    en_US: "rewriting",
    de_DE: "Häuser",
  });
  const current = words.find((entry) => entry.word === chosen[locale]) ?? words[0];
  if (!current) throw new Error("The explanation sample has no words.");
  return (
    <MonoWide>
      <div className="flx-toggles">
        <MonoSegmented
          label="Dictionary"
          value={locale}
          options={languageOptions}
          onChange={(value) => {
            if (isLocale(value)) setLocale(value);
          }}
        />
        <MonoSegmented
          label="Word"
          value={current.word}
          options={words.map((entry) => ({ value: entry.word, label: entry.word }))}
          onChange={(value) => {
            setChosen((previous) => ({ ...previous, [locale]: value }));
          }}
        />
      </div>
      <MonoFacing className="flx-single">
        <MonoSheet
          label={<span className="flx-word">{current.word}</span>}
          meta={`explain("${current.word}")`}
          aria-live="polite"
        >
          <Derivation entry={current} />
        </MonoSheet>
      </MonoFacing>
      <ExplainCaption locale={locale} />
    </MonoWide>
  );
}

/** The suggestions of one word, closest first, each with its edit distance. */
function SuggestionList({ suggestions }: { suggestions: Suggestion[] }) {
  if (suggestions.length === 0) return <p className="flx-none">No suggestion.</p>;
  return (
    <ol className="flx-suggestions">
      {suggestions.map((suggestion) => (
        <li key={suggestion.word}>
          <code>{suggestion.word}</code>
          <span>{suggestion.distance}</span>
        </li>
      ))}
    </ol>
  );
}

/** Whether a suggestion search finished, in words. */
const COMPLETENESS: Record<string, string> = {
  complete: "Complete: every candidate within reach was compared.",
  "edit-budget": "Stopped at the comparison budget; these are the best found until then.",
  "candidate-limit": "Stopped at the candidate limit; these are the best found until then.",
};
const completenessText = (value: string) => COMPLETENESS[value] ?? value;

function CheckedWord({ entry }: { entry: Checked }) {
  if (entry.correct) {
    return (
      <li className="flx-check">
        <code>{entry.word}</code>
        <span>correct</span>
      </li>
    );
  }
  return (
    <li className="flx-check" data-miss="">
      <code>{entry.word}</code>
      <span>not in the dictionary</span>
      {entry.suggestions !== undefined && (
        <div className="flx-check-detail">
          <SuggestionList suggestions={entry.suggestions} />
          <p className="flx-sub">{completenessText(entry.completeness ?? "")}</p>
        </div>
      )}
    </li>
  );
}

/** A tool's words, checked one by one, with suggestions for every miss. */
export function CheckSpecimen() {
  const [locale, setLocale] = useState<Locale>("en_US");
  const words = byLocale(samples.check, locale).words;
  const misses = words.filter((entry) => !entry.correct).length;
  return (
    <MonoWide>
      <div className="flx-toggles">
        <MonoSegmented
          label="Language"
          value={locale}
          options={languageOptions}
          onChange={(value) => {
            if (isLocale(value)) setLocale(value);
          }}
        />
      </div>
      <MonoFacing className="flx-single">
        <MonoSheet
          label={capitalized(`${spelled(words.length)} words, ${spelled(misses)} misspelled`)}
          meta="contains() · suggest()"
        >
          <ul className="flx-checks" aria-live="polite">
            {words.map((entry, index) => (
              <CheckedWord key={`${String(index)}:${entry.word}`} entry={entry} />
            ))}
          </ul>
        </MonoSheet>
      </MonoFacing>
      <MonoCaption>
        Real output of ferrolex-hunspell and ferrolex-suggest {renderedWith} with the LibreOffice{" "}
        {LANGUAGES[locale]} dictionary and the default limits. The number beside a suggestion is its
        edit distance. The program that wrote these results runs every check twice and stops if the
        two answers differ.
      </MonoCaption>
    </MonoWide>
  );
}

const BUDGET_LABELS: Record<string, string> = {
  small: "Small budget",
  default: "Default budget",
  large: "Larger budget",
};
const LIMIT_LABELS: Array<[keyof BudgetRun["limits"], ReactNode]> = [
  ["maxCandidates", "Dictionary words it may look at"],
  ["maxEditCells", "Letter comparisons it may make"],
  ["maxEditDistance", "Most edits a suggestion may need"],
  ["maxResults", "Most suggestions it returns"],
];

function budgetRun(id: string) {
  const run = samples.budget.runs.find((entry) => entry.id === id);
  if (!run) throw new Error(`The budget sample has no ${id} run.`);
  return run;
}
const standardRun = budgetRun("default");
const largeRun = budgetRun("large");
const largerBy = largeRun.limits.maxEditCells / standardRun.limits.maxEditCells;

/** One misspelling under three work budgets: what each found, and whether it finished. */
export function BudgetSpecimen() {
  const { runs, word } = samples.budget;
  const [id, setId] = useState("default");
  const current = runs.find((run) => run.id === id) ?? runs[0];
  if (!current) throw new Error("The budget sample has no runs.");
  return (
    <MonoWide>
      <div className="flx-toggles">
        <MonoSegmented
          label="Budget"
          value={current.id}
          options={runs.map((run) => ({ value: run.id, label: BUDGET_LABELS[run.id] ?? run.id }))}
          onChange={setId}
        />
      </div>
      <MonoFacing className="flx-facing" aria-live="polite">
        <MonoSheet label="The limits" meta="SuggestConfig">
          <dl className="flx-facts flx-limits">
            {LIMIT_LABELS.map(([key, label]) => (
              <Fact key={key} term={label}>
                {current.limits[key].toLocaleString("en-US")}
              </Fact>
            ))}
          </dl>
        </MonoSheet>
        <MonoSheet
          label={capitalized(`${spelled(current.suggestions.length)} suggestions`)}
          meta={`suggest("${word}")`}
        >
          <SuggestionList suggestions={current.suggestions} />
          <p className="flx-sub">{completenessText(current.completeness)}</p>
        </MonoSheet>
      </MonoFacing>
      <MonoCaption>
        Real output of ferrolex-suggest {renderedWith} with the LibreOffice English dictionary. The
        larger budget allows {largerBy.toLocaleString("en-US")} times the letter comparisons of the
        default, finishes the search, and returns the same {spelled(standardRun.suggestions.length)}{" "}
        suggestions.
      </MonoCaption>
    </MonoWide>
  );
}
