# VaultForge visual style rules

## Typography

- Choose typography by visual role. Use the CDS `Text` component with a `font`
  token such as `title3`, `body`, `label1`, or `caption`, and keep semantic
  heading elements through its `as` prop where appropriate.
- Use semantic CDS color props such as `fgMuted` for supporting text. Avoid
  global rules for `span`, `p`, or generic heading tags.
- Use monospace typography for addresses, hashes, signatures, and raw
  transaction data rather than ordinary interface copy.

## Layout and color

- Prefer CDS layout components and token-backed style props for spacing,
  alignment, and colors. Keep custom CSS for behavior or layout that the
  component API does not express, including the desktop shell and responsive
  scrollbars.
- Keep layout rules close to the component that owns them, and preserve
  responsive behavior when adjusting shared styles.
- Use semantic theme colors for status, danger, accent, and muted content;
  avoid hard-coded colors in application UI.
