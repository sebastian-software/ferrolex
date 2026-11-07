import { type ReactNode, useState } from "ardo/react";
import { ArdoActiveLink as NavLink, useArdoPageData, useArdoPath } from "ardo/runtime";
import {
  ArdoErrorBoundary,
  ArdoHeaderSearch,
  ArdoMobileNavigation,
  ArdoRoot,
  ArdoRootLayout,
  ArdoTOC,
} from "ardo/ui";
import { MarkDefs } from "ferramenta-family";
import {
  FerraGuideNavigation,
  FerraPage,
  FerraReadingLayout,
  FerraReadingTable,
  FerraTabs,
} from "ferramenta-family/ferra";
import touchIcon from "ferramenta-family/ferra-assets/ferrolex/apple-touch-icon.png?url";
import favicon from "ferramenta-family/ferra-assets/ferrolex/favicon.ico?url";
import { MonoFooter, MonoHeader, MonoPage } from "ferramenta-family/monografie";
import "ferramenta-family/ferra.css";
import "ferramenta-family/monografie.css";

import navigation from "./data/navigation.json";
import site from "./data/site.json";
import "ferramenta-family/ferra-ardo.css";

import "./components/site-shell.css";

export function Layout({ children }: { children: ReactNode }) {
  return (
    <ArdoRootLayout
      colorScheme="light"
      className="ferra-tokens ferra-ardo"
      icon={favicon}
      appleTouchIcon={touchIcon}
      iconBasePath={false}
    >
      {children}
    </ArdoRootLayout>
  );
}
export const ErrorBoundary = ArdoErrorBoundary;

function GuideNavigation({ onNavigate }: { onNavigate?: () => void }) {
  const path = useArdoPath();
  const first = navigation[0];
  if (!first) throw new Error("Documentation navigation is empty.");
  const current =
    navigation.find((section) => path === `/${section.id}` || path.startsWith(`/${section.id}/`)) ??
    first;
  return (
    <FerraGuideNavigation aria-label="Documentation pages">
      <NavLink to="/" onClick={onNavigate}>
        Overview
      </NavLink>
      <FerraTabs aria-label="Documentation sections">
        {navigation.map((section) => (
          <NavLink
            key={section.id}
            to={section.to}
            data-active={current.id === section.id}
            onClick={onNavigate}
          >
            {section.label}
          </NavLink>
        ))}
      </FerraTabs>
      <p>{current.label}</p>
      {current.pages.map(({ title, to: href }) => (
        <NavLink key={href} to={href} onClick={onNavigate}>
          {title}
        </NavLink>
      ))}
    </FerraGuideNavigation>
  );
}

/** The documentation menu and search, which stay in the header on documentation pages. */
function DocsActions() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <ArdoMobileNavigation title="Ferrolex documentation" open={open} onOpenChange={setOpen}>
        <GuideNavigation
          onNavigate={() => {
            setOpen(false);
          }}
        />
      </ArdoMobileNavigation>
      <ArdoHeaderSearch
        variant="icon"
        onActivate={() => {
          setOpen(false);
        }}
      />
    </>
  );
}

function Header({ docs }: { docs: boolean }) {
  return (
    <MonoHeader
      product="ferrolex"
      home="/"
      release={site.version}
      links={[
        <NavLink key="guide" to="/guide/getting-started">
          Guide
        </NavLink>,
        <NavLink key="reference" to="/reference/overview">
          Reference
        </NavLink>,
        <NavLink key="evidence" to="/evidence/compatibility">
          Evidence
        </NavLink>,
        <NavLink key="changelog" to="/changelog">
          Changelog
        </NavLink>,
        <a key="github" href="https://github.com/sebastian-software/ferrolex">
          GitHub
        </a>,
      ]}
      actions={docs ? <DocsActions /> : <ArdoHeaderSearch variant="icon" />}
    />
  );
}

function Footer() {
  return (
    <MonoFooter product="ferrolex" home="/">
      <a href="https://github.com/sebastian-software/ferrolex">GitHub</a>
      <a href="https://github.com/sebastian-software/ferrolex/releases">Releases</a>
      <a href="https://ardo-docs.dev">Built with Ardo</a>
    </MonoFooter>
  );
}

function Article({ children }: { children: ReactNode }) {
  const page = useArdoPageData();
  const path = useArdoPath();
  const current = navigation.find(
    (section) => path === `/${section.id}` || path.startsWith(`/${section.id}/`),
  );
  return (
    <FerraReadingLayout
      id="ferrolex-article"
      navigation={<GuideNavigation />}
      outline={<ArdoTOC source="dom" scope="#ferrolex-article" />}
    >
      <header>
        <p>Ferrolex / {current?.label ?? "Documentation"}</p>
        <h1>{page?.title.replaceAll("`", "") ?? "Documentation"}</h1>
      </header>
      {children}
    </FerraReadingLayout>
  );
}
const components = { table: FerraReadingTable, wrapper: Article };

/**
 * Every page wears the Monografie header and footer. The homepage reads on the
 * Monografie page ground; documentation keeps the Ferra reading layout until
 * the package's Monografie documentation layout replaces Article and DocsActions.
 */
export default function Root() {
  const path = useArdoPath();
  const home = path === "/" || path === "";
  const shell = (
    <>
      <MarkDefs />
      <ArdoRoot
        className="ferra-ardo-shell"
        scrollMode="document"
        header={<Header docs={!home} />}
        sidebar={false}
        footer={false}
        components={components}
      />
      <Footer />
    </>
  );
  return home ? (
    <MonoPage className="flx-home">{shell}</MonoPage>
  ) : (
    <FerraPage className="ferra-site flx-docs">{shell}</FerraPage>
  );
}
