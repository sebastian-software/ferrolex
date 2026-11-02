import { defineArdoConfig } from "ardo/config";

export default defineArdoConfig({
  title: "Ferrolex",
  description:
    "An independent Rust spell checker for Hunspell dictionaries, deterministic suggestions and compiled dictionary artifacts. Pre-1.0, with documented compatibility boundaries.",
  base: "/",
  siteUrl: "https://ferrolex.dev",
  icons: false,
  markdown: { headingAttributes: true },
  linkCheck: { level: "error" },
});
