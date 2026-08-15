# Document Studio Figma Handoff

This folder is an implementation-ready handoff, not a live `.fig` file.

## Build order

1. Create the page structure in `FILE-STRUCTURE.md`.
2. Import `../packages/tokens/document-studio.tokens.json` into Tokens Studio.
3. Create Figma variables for color modes, spacing, radius, typography and motion.
4. Build primitives and components from `COMPONENT-INVENTORY.md` using Auto Layout.
5. Recreate the Home and Workbench frames from the reference screenshots in `screens/`.
6. Add interactions from `PROTOTYPE-FLOWS.md`.
7. Run Design Lint and accessibility checks before handoff.

Use the UI UX Pro Max skill through the official `uipro-cli` installation instructions in `../scripts/install-ui-ux-pro-max.*`.
