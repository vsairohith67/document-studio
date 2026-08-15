# Figma Handoff

## File pages

1. Cover & decisions
2. Foundations
3. Variables and tokens
4. Components
5. Patterns
6. Desktop flows
7. Web adaptation
8. Mobile capture
9. Prototypes
10. Accessibility and redlines
11. Archive

## Variable collections

- `Color / Light`
- `Color / Dark`
- `Color / High Contrast`
- `Spacing`
- `Radius`
- `Typography`
- `Elevation`
- `Motion`
- `Density`

## Core components

App shell, navigation item, command search, drop zone, tool card, file row, page thumbnail, canvas toolbar, inspector section, form controls, segmented control, locality badge, status chip, progress row, job tray, result summary, empty/error state, toast and dialog.

## Plugins/workflow

- Tokens Studio: import/sync token JSON.
- Iconify: explore/import icons; verify each icon-set license and normalize to a small chosen set.
- Design Lint: find missing styles/inconsistent values.
- Accessibility checker: contrast and naming checks.
- Autoflow/equivalent: document user-flow arrows only, not production components.

## Figma-to-code rule

Figma is the design source for behavior/layout and components; tokens JSON is the shared source for numeric values. Do not copy absolute pixel positions from screenshots into production code.
