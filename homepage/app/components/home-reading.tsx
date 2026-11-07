import { ArdoLink as Link } from "ardo/runtime";
import { MonoCredit, MonoLeadIn, MonoPassage, MonoSection } from "ferramenta-family/monografie";

import { dictionary, samples } from "./home-data";
import { BudgetSpecimen, CheckSpecimen, ExplainSpecimen } from "./lex-specimens";

/* The homepage's reading chapters: what a Hunspell dictionary is, checking
   and suggesting, and the budget every suggestion search keeps. */

const english = dictionary("en_US");

export function Explainer() {
  return (
    <MonoSection id="overview" title="A dictionary is a word list and its rules.">
      <MonoPassage
        note={{
          term: "Hunspell",
          children:
            "The open-source spell checker used by LibreOffice, Firefox and Chrome. Dictionaries for many languages are written in its format.",
        }}
      >
        <p>
          <MonoLeadIn>A spell checker does not store</MonoLeadIn> every word a language can form.
          English alone has walk, walks, walked and walking; German and Hungarian build far more
          forms from one word. A Hunspell dictionary comes as two files instead: a word list, in
          which each word carries a few letters that name the rules it takes, and a rules file that
          says what those rules add or remove.
        </p>
      </MonoPassage>
      <MonoPassage
        note={{
          side: "right",
          term: "Affix",
          children:
            "A piece added at the start (a prefix, like un-) or at the end (a suffix, like -ing) to form another word.",
        }}
      >
        <p>
          The LibreOffice English dictionary stores {english.stems.toLocaleString("en-US")} words
          and reaches many more through its rules. To check a word, Ferrolex looks for a stored word
          and a rule that leads from it to the spelling in front of it. Choose a word below to see
          the path it found, or why it found none.
        </p>
      </MonoPassage>
      <ExplainSpecimen />
      <MonoCredit>
        <Link to="/reference/overview/">What the engine and its command line read</Link>
      </MonoCredit>
    </MonoSection>
  );
}

export function Checking() {
  return (
    <MonoSection
      id="example"
      title="Check the words. Get the same suggestions every time."
      standfirst="Your tool hands over the words it wants checked. Ferrolex says which ones the dictionary accepts and suggests corrections for the rest, in the same order on every machine."
    >
      <MonoPassage
        note={{
          term: "Edit distance",
          children:
            "The number of single changes that turn one spelling into another: a letter added, removed, replaced, or two neighbors swapped.",
        }}
      >
        <p>
          <MonoLeadIn>Suggestions are ranked</MonoLeadIn> by how few changes they need, weighed with
          the dictionary’s own hints about likely typos, then alphabetically. Nothing depends on the
          machine or the operating system, so a test that expects a suggestion today still finds it
          tomorrow and in CI.
        </p>
      </MonoPassage>
      <p>
        Ferrolex does not split text into words; the editor, linter or CI check that calls it knows
        its format and decides what counts as a word. Switch between the two languages below to see
        what the released engine answers for a short sentence in each.
      </p>
      <CheckSpecimen />
      <MonoCredit>
        <Link to="/guide/getting-started/">Check your first word</Link>
      </MonoCredit>
    </MonoSection>
  );
}

export function Limits() {
  const { word } = samples.budget;
  return (
    <MonoSection
      id="limits"
      title="Every search has a budget, and says when it ran out."
      standfirst="A large dictionary holds hundreds of thousands of words. Ferrolex caps the work a suggestion may take and reports whether the search finished."
    >
      <MonoPassage
        note={{
          side: "right",
          term: "Budget",
          children:
            "The limits a caller sets: how many dictionary words a search may look at, how many letter comparisons it may make, and how far a suggestion may stray.",
        }}
      >
        <p>
          <MonoLeadIn>An editor checks as you type</MonoLeadIn>, so one misspelling must not stall
          it. Ferrolex never expands a whole dictionary in advance and stops at the limits you set.
          Its answer then carries a plain statement: complete, or stopped at a named limit with the
          best suggestions found until then.
        </p>
      </MonoPassage>
      <p>
        Below, the same misspelling, <code>{word}</code>, is searched under three budgets. The
        default budget stops before it has seen every candidate and says so; a larger one finishes
        the search. You choose which is right for your tool.
      </p>
      <BudgetSpecimen />
      <MonoCredit>
        <Link to="/reference/overview/#try-it">The suggestion limits on the command line</Link>
      </MonoCredit>
    </MonoSection>
  );
}
