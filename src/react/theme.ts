import type { ThemeConfig } from "@coinbase/cds-web/core/theme";
import { defaultTheme } from "@coinbase/cds-web/themes/defaultTheme";

const ubuntuMono = "'Ubuntu Mono', 'DejaVu Sans Mono', Menlo, Consolas, monospace";

const ubuntuMonoFamily = {
  display1: ubuntuMono,
  display2: ubuntuMono,
  display3: ubuntuMono,
  title1: ubuntuMono,
  title2: ubuntuMono,
  title3: ubuntuMono,
  title4: ubuntuMono,
  headline: ubuntuMono,
  body: ubuntuMono,
  label1: ubuntuMono,
  label2: ubuntuMono,
  caption: ubuntuMono,
  legal: ubuntuMono,
} as const;

export const vaultForgeTheme = {
  ...defaultTheme,
  id: "vaultforge-ubuntu-mono",
  fontFamily: ubuntuMonoFamily,
  fontFamilyMono: ubuntuMonoFamily,
} as const satisfies ThemeConfig;
