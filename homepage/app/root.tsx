import { type ReactNode, useState } from "ardo/react";
import { ArdoActiveLink as NavLink, useArdoHref, useArdoPageData, useArdoPath } from "ardo/runtime";
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
  FerraFooter,
  FerraGuideNavigation,
  FerraHeader,
  FerraPage,
  FerraReadingLayout,
  FerraReadingTable,
  FerraTabs,
} from "ferramenta-family/ferra";
import touchIcon from "ferramenta-family/ferra-assets/ferrolex/apple-touch-icon.png?url";
import favicon from "ferramenta-family/ferra-assets/ferrolex/favicon.ico?url";
import { WORKSHOP } from "ferramenta-family/registry";
import "ferramenta-family/ferra.css";

import navigation from "./data/navigation.json";
import site from "./data/site.json";
import "ferramenta-family/ferra-ardo.css";

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

function Header() {
  const home = useArdoHref("/");
  const [open, setOpen] = useState(false);
  return (
    <FerraHeader
      product="ferrolex"
      home={home}
      nav={
        <>
          <NavLink to="/guide/getting-started">Guide</NavLink>
          <NavLink to="/reference/overview">Reference</NavLink>
          <NavLink to="/evidence/compatibility">Evidence</NavLink>
          <a href="https://ferramenta.dev">Ferramenta</a>
        </>
      }
      actions={
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
      }
    />
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

export default function Root() {
  const home = useArdoHref("/");
  return (
    <FerraPage className="ferra-site">
      <MarkDefs />
      <ArdoRoot
        className="ferra-ardo-shell"
        scrollMode="document"
        header={<Header />}
        sidebar={false}
        footer={false}
        components={components}
      />
      <FerraFooter product="ferrolex" home={home}>
        <p>
          Ferrolex v{site.version} · MIT / Apache-2.0 License ·{" "}
          <a href="https://ardo-docs.dev">Built with Ardo</a> ·{" "}
          <a href={WORKSHOP.openSource}>{WORKSHOP.company}</a> ·{" "}
          <a href="https://ferramenta.dev">Ferramenta family</a>
        </p>
      </FerraFooter>
    </FerraPage>
  );
}
