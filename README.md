# Document Studio

**Document Studio** is a privacy-first, high-performance document workspace planned for desktop first, followed by an optional web service and mobile capture companion.

This repository is a **Codex-ready architecture and implementation starter**. It contains the product blueprint, feature catalogue, UI/UX handoff, design tokens, architecture diagrams, a working static prototype, operation contracts, test strategy, model registry, dependency register, Notion import pack, and the first implementation prompts.

## Re-audit status

This package replaces the older working title and closes the documentation gaps found during the 22 July 2026 review:

- The canonical product name is **Document Studio** everywhere.
- The platform strategy is explicit: Windows desktop first, macOS second, optional web third, mobile capture fourth.
- Desktop orchestration uses the Tauri Rust core; a Go service is reserved for the future cloud control plane rather than adding a second local runtime prematurely.
- The feature catalogue contains **132 planned capabilities** in one complete edition.
- The UI/UX system, Figma handoff, accessibility rules, performance budgets, security model, database schema, API contracts, Codex prompts, dependency/model registers, and import instructions are included.
- Existing source research, prior PDF/DOCX reports, diagrams, prototype screenshots, and whiteboard exports are copied into the Notion import pack.

## Start here

1. Read `CODEX_START_HERE.md`.
2. Read `docs/00-AUDIT-AND-COMPLETION.md` and `docs/01-PRODUCT-CHARTER.md`.
3. Open `apps/prototype/index.html` to inspect the UI concept.
4. Use `codex/prompts/01-foundation.md` for the first Codex run.
5. Do not start feature work until Phase 0 acceptance tests pass.
6. Read `docs/23-DEVELOPMENT-SETUP.md` for the target Windows toolchain and first build.
7. Read `FINAL_RECHECK.md` for the verified completion state and remaining external actions.

## Build philosophy

- Local-first and offline by default.
- Never overwrite originals without explicit confirmation.
- Never report success until the output has been reopened and verified.
- Prefer simple, mature engines over novel dependencies.
- Keep cloud and external AI optional and visible.
- Build every capability through the same typed operation contract.
- Performance is a product requirement, not a late optimization task.

## Package map

- `docs/` - product, system, security, UX, testing and delivery documentation.
- `apps/desktop/` - Tauri + React starter scaffold.
- `apps/prototype/` - browser-openable visual prototype.
- `services/cloud-control-plane/` - optional future Go service skeleton.
- `packages/contracts/` - JSON Schema and TypeScript operation contracts.
- `packages/tokens/` - design tokens for code and Figma.
- `figma/` - Figma page/component/variable handoff.
- `models/` - approved and evaluation-only local model register.
- `diagrams/` - source and exported architecture/flow diagrams.
- `codex/prompts/` - staged implementation prompts.
- `notion-import/` - import-ready project knowledge base and attachments.
- `report/` - printable master DOCX and PDF.
- `.github/`, `CONTRIBUTING.md`, `SECURITY.md` - repository workflow, issue/PR templates and security reporting.
- `CHANGELOG.md`, `THIRD_PARTY_NOTICES.md` - release history and dependency/model adoption gate.

Version: `2.0.1-final-recheck`  
Audit date: `22 July 2026`

## Goal Mode build workflow

The canonical Codex execution method is documented in `docs/25-GOAL-MODE-BUILD-PLAYBOOK.md`. Start with `codex/goals/G00-readiness-audit.md` in `/plan`, then run one bounded `/goal` per milestone from `codex/goals/`.
