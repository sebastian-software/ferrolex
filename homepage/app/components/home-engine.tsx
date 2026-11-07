import { ArdoLink as Link } from "ardo/runtime";
import {
  MonoCredit,
  MonoLeadIn,
  MonoPassage,
  MonoProofs,
  MonoSection,
  MonoTerms,
  MonoWide,
} from "ferramenta-family/monografie";

import { version } from "../data/site.json";
import {
  compatibility,
  dictionary,
  ferrolex,
  lowerFirst,
  registryNote,
  samples,
  spelled,
} from "./home-data";

/* The homepage's engine chapters: dictionary data, compatibility, the field
   and what Ferrolex leaves to your tool. */

const english = dictionary("en_US");
const { locales } = compatibility;
const ready = locales.filter((entry) => entry.state === "ready");
const pending = locales.filter((entry) => entry.state !== "ready");
const STATE_LABELS: Record<string, string> = {
  ready: "Ready",
  progress: "In progress",
  blocked: "Blocked",
};

/** Names in a sentence: "a", "a and b", "a, b and c". */
const listed = (names: string[]) =>
  names.length < 2 ? names.join("") : `${names.slice(0, -1).join(", ")} and ${names.at(-1)}`;

const DICTIONARY_TERMS = [
  {
    term: "Installed on request",
    description: (
      <>
        <code>ferrolex dictionary install</code> downloads a pair into a folder you name and refuses
        any file whose bytes differ from the catalog.
      </>
    ),
  },
  {
    term: "A license for each language",
    description: `Every entry names its reviewed license and links the upstream notice. The English dictionary is ${english.license}, for example; the engine itself stays MIT or Apache-2.0.`,
  },
  {
    term: "Compiled for shipping",
    description:
      "Compile a word list or a Hunspell pair into one file, then validate that file before you load it.",
  },
  {
    term: "Your own words",
    description:
      "Layer a project word list over any dictionary. The command line picks up .ferrolex/words.txt on its own.",
  },
];

export function Dictionaries() {
  const note = registryNote("A dictionary you can ship");
  return (
    <MonoSection
      id="dictionaries"
      title="Choose the data you ship."
      standfirst={`Ferrolex bundles no dictionary. A reviewed catalog names ${spelled(samples.catalog.locales)} LibreOffice dictionaries, each pinned to its exact bytes, and installs one only when you ask.`}
    >
      <MonoPassage note={{ term: note.title, children: note.body }}>
        <p>
          <MonoLeadIn>Dictionaries are data</MonoLeadIn> with their own authors and licenses, and a
          product that ships one has to know which version it ships. Ferrolex keeps that choice with
          you: it never downloads or updates a dictionary while it checks, and every file it
          installs must match the digest recorded in the catalog.
        </p>
      </MonoPassage>
      <MonoWide>
        <MonoTerms prose columns={2} items={DICTIONARY_TERMS} />
      </MonoWide>
      <MonoCredit>
        <Link to="/guide/dictionaries/">How dictionary installation works</Link>
      </MonoCredit>
    </MonoSection>
  );
}

/** The pending locales in a sentence: "Hungarian is in progress". */
const pendingText = listed(
  pending.map(
    (entry) =>
      `${entry.name.replace(/ \(.*\)$/, "")} is ${STATE_LABELS[entry.state]?.toLowerCase() ?? entry.state}`,
  ),
);

/** Each tested locale with its status, from the README's generated block. */
function LocaleStatus() {
  return (
    <MonoWide>
      <MonoProofs
        className="flx-locales"
        items={locales.map((entry) => ({
          label: (
            <>
              {entry.name} <code>{entry.locale}</code>
            </>
          ),
          value: STATE_LABELS[entry.state] ?? entry.status,
        }))}
      />
    </MonoWide>
  );
}

export function Compatibility() {
  return (
    <MonoSection
      id="compatibility"
      title="Measured locale by locale."
      standfirst="Ferrolex is alpha. Instead of claiming Hunspell compatibility in general, the project tests each dictionary it supports and publishes the result."
    >
      <MonoPassage
        note={{
          term: "Strict import",
          children:
            "Reading a dictionary so that any rule Ferrolex does not support stops the import, instead of being skipped quietly.",
        }}
      >
        <p>
          <MonoLeadIn>For every locale in the test set</MonoLeadIn>, the exact upstream dictionary
          is imported strictly and a reviewed list of word forms is checked against it. CI repeats
          this on relevant changes and before every release. {spelled(ready.length)} of the{" "}
          {spelled(locales.length)} locales pass; {pendingText}.
        </p>
      </MonoPassage>
      <p>
        Passing these checks is not full Hunspell parity. German shows the gap: the dictionary
        imports, and ordinary words and their forms work, but in Ferrolex {version} compound words
        such as Wörterbuch are not recognized from this dictionary yet, as the first example above
        shows.
      </p>
      <LocaleStatus />
      <MonoCredit>
        From the status the repository generates for its README.{" "}
        <Link to="/evidence/compatibility/">Every locale, its source and its known limits</Link>
      </MonoCredit>
    </MonoSection>
  );
}

export function Field() {
  return (
    <MonoSection
      id="field"
      title="Where it sits among spell checkers."
      standfirst="Several libraries read Hunspell dictionaries. Ferrolex is an independent Rust engine with bounded, repeatable suggestions, compiled dictionaries and a digest-pinned catalog."
    >
      <MonoWide>
        <MonoTerms
          prose
          columns={3}
          items={[
            {
              term: <a href="https://github.com/hunspell/hunspell">Hunspell</a>,
              description:
                "The C++ library and command-line tool that defined the format, used by LibreOffice, Firefox and Chrome. Ferrolex checks its own accept and reject decisions against it.",
            },
            {
              term: <a href="https://github.com/wooorm/nspell">nspell</a>,
              description:
                "A JavaScript spell checker for Hunspell dictionaries that covers much of Hunspell’s core, for browsers and Node.js.",
            },
            {
              term: <a href="https://github.com/helix-editor/spellbook">Spellbook</a>,
              description:
                "A Rust library compatible with the Hunspell dictionary format, from the team behind the Helix editor.",
            },
          ]}
        />
      </MonoWide>
      <MonoCredit>
        Many more spell checkers exist, from Aspell to those built into operating systems.{" "}
        <a href="https://en.wikipedia.org/wiki/Spell_checker">An overview of the field</a>
      </MonoCredit>
    </MonoSection>
  );
}

const INSTALL =
  "cargo install --locked --git https://github.com/sebastian-software/ferrolex ferrolex-cli";

export function Fit() {
  const note = registryNote("A feature we chose to remove");
  return (
    <MonoSection id="fit" title="A focused engine. Your tool around it.">
      <MonoPassage note={{ side: "right", term: note.title, children: note.body }}>
        <p>
          Ferrolex is {lowerFirst(ferrolex.what)}. It {lowerFirst(ferrolex.does)}{" "}
          {ferrolex.audience} It owns the dictionaries, the checks and the suggestions; your tool
          knows its own format and decides which words to check.
        </p>
      </MonoPassage>
      <MonoWide>
        <MonoTerms
          prose
          columns={2}
          items={[
            {
              term: "Rust library and command line",
              description:
                "Import dictionaries, check words, explain a verdict, suggest corrections and compile dictionaries, from Rust or from the ferrolex command.",
            },
            {
              term: "Not on crates.io yet",
              description: (
                <>
                  Releases are tagged on GitHub. Install the command line from the repository:{" "}
                  <code className="flx-command">{INSTALL}</code>
                </>
              ),
            },
            {
              term: "Node.js",
              description:
                "A binding is built and tested with every release, but it is not published to npm yet. C, Python and editor integrations are prototypes.",
            },
            {
              term: "Not included",
              description:
                "Splitting documents or source code into words, and editor protocols. Format-aware tools extract the words and call Ferrolex.",
            },
          ]}
        />
      </MonoWide>
      <MonoCredit>
        <Link to="/evidence/product-boundary/">Why parsing stays outside</Link>
      </MonoCredit>
    </MonoSection>
  );
}
