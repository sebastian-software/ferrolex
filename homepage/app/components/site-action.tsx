import type { ComponentProps } from "ardo/react";

import { useArdoHref } from "ardo/runtime";
import { FerraAction } from "ferramenta-family/ferra";

// Ardo owns deployment paths; the family component stays framework-independent.
export function SiteAction({
  to,
  ...props
}: { to: string } & Omit<ComponentProps<typeof FerraAction>, "href">) {
  return <FerraAction {...props} href={useArdoHref(to)} />;
}
