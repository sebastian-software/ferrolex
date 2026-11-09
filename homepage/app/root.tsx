import type { ReactNode } from "ardo/react";

import { ArdoActiveLink as NavLink } from "ardo/runtime";
import {
  MonoArdoErrorBoundary,
  MonoArdoLayout,
  MonoArdoSite,
  type MonoDocsSection,
} from "ferramenta-family/ardo";
import touchIcon from "ferramenta-family/ferra-assets/ferrolex/apple-touch-icon.png?url";
import favicon from "ferramenta-family/ferra-assets/ferrolex/favicon.ico?url";

import navigation from "./data/navigation.json";
import site from "./data/site.json";
import "ferramenta-family/monografie.css";
import "ferramenta-family/monografie-docs.css";
import "ferramenta-family/monografie-ardo.css";

export function Layout({ children }: { children: ReactNode }) {
  return (
    <MonoArdoLayout icon={favicon} appleTouchIcon={touchIcon}>
      {children}
    </MonoArdoLayout>
  );
}
export const ErrorBoundary = MonoArdoErrorBoundary;

const sections: MonoDocsSection[] = navigation.map((section) => ({
  id: section.id,
  label: section.label,
  to: section.to,
  pages: section.pages.map(({ title, to }) => ({ label: title, to })),
}));

const links = [
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
];

/** Every page on Monografie's Ardo shell; the documentation adds its menu, search and reading layout. */
export default function Root() {
  return (
    <MonoArdoSite
      product="ferrolex"
      name="Ferrolex"
      release={site.version}
      links={links}
      repository="https://github.com/sebastian-software/ferrolex"
      sections={sections}
      homeClassName="flx-home"
      docsClassName="flx-docs"
      footer={
        <>
          <a href="https://github.com/sebastian-software/ferrolex">GitHub</a>
          <a href="https://github.com/sebastian-software/ferrolex/releases">Releases</a>
          <a href="https://ardo-docs.dev">Built with Ardo</a>
        </>
      }
    />
  );
}
