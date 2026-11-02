import type { ArdoMetaFunction } from "ardo/runtime";

import { ArdoLink as Link } from "ardo/runtime";
import { Mark, RegistryFacts } from "ferramenta-family";
import {
  FerraArticle,
  FerraChoices,
  FerraCode,
  FerraColumns,
  FerraEditorialRow,
  FerraFeatureGrid,
  FerraInstrument,
  FerraNotes,
  FerraProductLead,
  FerraProductPage,
  FerraReleaseFacts,
} from "ferramenta-family/ferra";
import { family, WORKSHOP } from "ferramenta-family/registry";

import { SiteAction } from "../components/site-action";
import site from "../data/site.json";

// oxlint-disable-next-line react/only-export-components -- Ardo reads route metadata.
export const meta: ArdoMetaFunction = () => [
  { title: "Ferrolex — Your dictionaries. A Rust spell-checking engine." },
  {
    name: "description",
    content:
      "Check words and produce deterministic suggestions with existing Hunspell dictionaries. An independent Rust implementation, with explicit pre-1.0 compatibility boundaries.",
  },
];
// oxlint-disable-next-line react/only-export-components -- Ardo reads the route layout.
export const handle = { layout: "bare" };
const member = family.find((entry) => entry.name === "ferrolex");
if (!member) throw new Error("Ferrolex is missing from the family registry.");
const tool = { ...member, version: site.version };

function Opening() {
  return (
    <FerraProductLead
      title="Your dictionaries. A Rust spell-checking engine."
      introduction="Check words and produce deterministic suggestions with existing Hunspell dictionaries. Ferrolex implements the engine in Rust, without linking the native Hunspell library. Use it from your application or the command line."
      art={<FerraInstrument product="ferrolex" />}
      byline={
        <>
          An open-source project by <a href={WORKSHOP.openSource}>{WORKSHOP.company}</a>.
        </>
      }
    >
      <div className="ferra-actions">
        <SiteAction to="/guide/getting-started/">Try Ferrolex</SiteAction>
        <SiteAction variant="secondary" to="/evidence/compatibility/">
          Check compatibility
        </SiteAction>
      </div>
      <p>
        Pre-1.0. Public APIs may change in minor releases. Compatibility is documented per
        dictionary; a successful download is not a promise of full Hunspell recognition.
      </p>
    </FerraProductLead>
  );
}

function Highlights() {
  return (
    <FerraColumns>
      <FerraArticle
        title="Keep the dictionary knowledge."
        icon={<Mark name="document" size={28} />}
        meta={<FerraReleaseFacts tool={tool} />}
      >
        <p>
          Hunspell dictionaries carry years of spelling rules. Ferrolex reads supported affix and
          compound rules, checks words and produces bounded, deterministic suggestions.
        </p>
        <Link to="/evidence/compatibility/">Read the reviewed locale evidence</Link>
      </FerraArticle>
      <FerraArticle
        title="Choose the data you ship."
        icon={<Mark name="package" size={28} />}
        meta={<Link to="/guide/dictionaries/">How dictionary acquisition works</Link>}
      >
        <p>
          A reviewed catalog pins upstream sources, digests and license notices. Downloads go into a
          cache you select. Ordinary checks do not fetch or update dictionaries implicitly.
        </p>
        <Link to="/guide/getting-started/">Start with a word list or Hunspell pair</Link>
      </FerraArticle>
    </FerraColumns>
  );
}

function Scope() {
  return (
    <FerraEditorialRow
      notes={<FerraNotes label="Inside Ferrolex" notes={tool.editorial?.notes ?? []} />}
    >
      <FerraArticle
        title="A focused engine for your application."
        introduction="Your tool understands its document format. Ferrolex handles the dictionary work."
        meta={<Link to="/evidence/product-boundary/">Why parsing stays outside</Link>}
      >
        <FerraFeatureGrid
          items={[
            {
              heading: "Recognition and suggestions",
              text: "Check words and get repeatable suggestions with explicit work limits and completeness reporting.",
            },
            {
              heading: "Compiled dictionaries",
              text: "Compile supported word lists or Hunspell pairs into standalone artifacts, then validate their format before use.",
            },
            {
              heading: "Application-owned parsing",
              text: "Extract prose and identifiers in the tools that understand your format. Pass the words to the Rust API.",
            },
            {
              heading: "An explicit early-stage contract",
              text: "Rust and the CLI are available. The Node.js binding is implemented but not yet published to npm; editor and other runtime integrations remain prototypes.",
            },
          ]}
        />
      </FerraArticle>
    </FerraEditorialRow>
  );
}

function GettingStarted() {
  return (
    <FerraChoices>
      <FerraArticle
        title="Add it to Rust."
        introduction="Import dictionaries and request suggestions through the public library API."
      >
        <FerraCode caption="Install the library">cargo add ferrolex</FerraCode>
        <SiteAction to="/reference/overview/#rust-library-quick-start">
          Read the Rust quick start
        </SiteAction>
      </FerraArticle>
      <FerraArticle
        title="Try the command line."
        introduction="Start with a plain word list, then choose the dictionary sources your workflow needs."
      >
        <FerraCode caption="Install the CLI">cargo install --locked ferrolex-cli</FerraCode>
        <SiteAction variant="secondary" to="/guide/getting-started/">
          Check your first word
        </SiteAction>
      </FerraArticle>
    </FerraChoices>
  );
}

export default function HomePage() {
  return (
    <RegistryFacts>
      <FerraProductPage>
        <Opening />
        <Highlights />
        <Scope />
        <GettingStarted />
      </FerraProductPage>
    </RegistryFacts>
  );
}
