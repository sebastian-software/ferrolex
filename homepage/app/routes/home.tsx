import type { ArdoMetaFunction } from "ardo/runtime";

import { Fragment } from "ardo/react";
import { ArdoLink as Link } from "ardo/runtime";
import {
  MonoAction,
  MonoClosing,
  MonoColophon,
  MonoEngraving,
  MonoOpening,
} from "ferramenta-family/monografie";
import { WORKSHOP } from "ferramenta-family/registry";

import { compatibility, ferrolex, lowerFirst, samples } from "../components/home-data";
import { Compatibility, Dictionaries, Field, Fit } from "../components/home-engine";
import { Checking, Explainer, Limits } from "../components/home-reading";
import { rustVersion, version } from "../data/site.json";

// oxlint-disable-next-line react/only-export-components -- Ardo reads route metadata.
export const meta: ArdoMetaFunction = () => [
  { title: "Ferrolex — Your dictionaries. A Rust spell checker." },
  {
    name: "description",
    content:
      "An independent Rust spell checker for Hunspell dictionaries: checks words, explains its verdicts and gives the same bounded suggestions every time. Alpha, with compatibility measured locale by locale.",
  },
];
// oxlint-disable-next-line react/only-export-components -- Ardo reads the route layout.
export const handle = { layout: "bare" };

const releaseLine = version.split(".").slice(0, 2).join(".");
const ready = compatibility.locales.filter((entry) => entry.state === "ready").length;

function Actions() {
  return (
    <>
      <MonoAction href="/guide/getting-started/" primary detail="The command line and Rust">
        Get started
      </MonoAction>
      <MonoAction href="/evidence/compatibility/" detail="Locale by locale">
        Check compatibility
      </MonoAction>
    </>
  );
}

function Opening() {
  return (
    <MonoOpening
      product="ferrolex"
      instrumentLabel="Ferrolex's instrument: a long steel gauge holding four engraved type blocks, A, F, N and E, under a stepped bridge. A rust-brown sliding stop rests on the bridge between two blocks, and a spare rust-brown insert stands upright at the end."
      kicker={`Ferrolex ${releaseLine}, ${lowerFirst(ferrolex.what)} for Rust`}
      title="Your dictionaries. A Rust spell checker."
      subtitle="Hunspell’s dictionaries, read by an independent engine."
      standfirst="Editors, linters and CI checks all need to know whether a word is spelled right, and what was probably meant when it is not. Open dictionaries for many languages come in the Hunspell format: a word list plus a file of spelling rules. Ferrolex is a Rust library and command-line tool that reads those dictionaries, checks words and suggests corrections, without linking the Hunspell C++ library. It is alpha: the interfaces may still change, and its compatibility is measured locale by locale."
      actions={<Actions />}
      facts={[
        {
          value: <em>{samples.catalog.locales}</em>,
          label: "LibreOffice dictionaries in the reviewed catalog",
        },
        {
          value: (
            <>
              <em>{ready}</em> of {compatibility.locales.length}
            </>
          ),
          label: "tested locales that pass their strict import and word checks",
        },
        { value: "Same", label: "suggestions in the same order on every machine" },
        { value: `Rust ${rustVersion}+`, label: "MIT or Apache-2.0, alpha" },
      ]}
    />
  );
}

// Keys are written out: minified builds do not keep function names.
const chapters = [
  { key: "explainer", Chapter: Explainer },
  { key: "checking", Chapter: Checking },
  { key: "limits", Chapter: Limits },
  { key: "dictionaries", Chapter: Dictionaries },
  { key: "compatibility", Chapter: Compatibility },
  { key: "field", Chapter: Field },
  { key: "fit", Chapter: Fit },
];

/** Where Ferrolex comes from, and who makes it. */
function Origin() {
  return (
    <section aria-label="Origin">
      <div className="mono-axis">
        <MonoColophon>
          {ferrolex.succeeds} set the dictionary standard. Ferrolex reads those dictionaries with
          its own engine, written in Rust. It is one of the Ferramenta engines, made by{" "}
          {WORKSHOP.name} in {WORKSHOP.place}.
        </MonoColophon>
      </div>
    </section>
  );
}

export default function HomePage() {
  return (
    <>
      <Opening />
      {chapters.map(({ key, Chapter }, index) => (
        <Fragment key={key}>
          <MonoEngraving seed={index + 1} />
          <Chapter />
        </Fragment>
      ))}
      <MonoEngraving seed={chapters.length + 1} />
      <Origin />
      <MonoClosing
        id="start"
        title="Start with the words you have."
        standfirst="Check a plain word list first, then install the Hunspell dictionary your users write in. Both work from the command line and from Rust."
        actions={<Actions />}
        links={
          <>
            <a href="https://github.com/sebastian-software/ferrolex">GitHub repository</a>
            <Link to="/guide/getting-started/">Documentation</Link>
            <Link to="/changelog/">Changelog</Link>
            <a href={WORKSHOP.consulting}>Integration and support</a>
          </>
        }
      />
    </>
  );
}
