# Document Studio — Codex Goal Mode Build Playbook

**Status:** Approved execution method  
**Product:** Document Studio  
**Purpose:** Replace repetitive micro-prompts with persistent, measurable Codex goals while preserving engineering gates.

## Executive decision

Use a **Plan → Goal → Verify → Review → Merge** workflow.

- Use `/plan` when the milestone still needs investigation, sequencing, or clarification.
- After the plan is accepted, use `/goal` with a precise outcome, constraints, and measurable completion criteria.
- Keep all work for one milestone in the same goal chat.
- Start a new chat and branch/worktree for the next independent milestone.
- Do **not** create one giant goal for all 132 capabilities. Each goal must end in a runnable, testable, reviewable result.

This is a hybrid approach: Goal Mode provides autonomy inside a milestone; milestone boundaries protect architecture, quality, usage, and rollback.

## Operating model

```text
Repository readiness
        ↓
/plan — inspect, clarify, propose file-level plan
        ↓ human accepts or edits plan
/goal — persistent milestone objective
        ↓
Inspect → Implement → Test → Verify → Document
        ↓
Acceptance gate passes?
    ├── No: continue in the same goal chat
    └── Yes: review diff → commit/PR → merge
                              ↓
                    next goal in a new chat/worktree
```

## Thread and branch policy

| Goal | Chat | Branch/worktree | Environment |
|---|---|---|---|
| G00 Readiness audit | `DS-G00 — Readiness` | `main`, read-only | Local |
| G01 Foundation | `DS-G01 — Foundation` | `feat/g01-foundation` | Worktree after Git setup |
| G02 Merge PDF | `DS-G02 — PDF Merge` | `feat/g02-pdf-merge` | Worktree |
| G03 Core PDF and viewer | `DS-G03 — Core PDF` | `feat/g03-core-pdf` | Worktree |
| G04 Optimize and convert | `DS-G04 — Optimize Convert` | `feat/g04-optimize-convert` | Worktree |
| G05 OCR and safety | `DS-G05 — OCR Safety` | `feat/g05-ocr-safety` | Worktree |
| G06 Workbench, forms and signing | `DS-G06 — Workbench` | `feat/g06-workbench` | Worktree |
| G07 Automation, optional AI and hardening | `DS-G07 — Automation AI` | `feat/g07-automation-ai` | Worktree |

Continue in the same chat for fixes, clarification, test failures, and acceptance work within that goal. Start a separate side chat for explanations or status summaries that should not interrupt the main goal.

## Model and reasoning policy

| Work | Preferred model | Reasoning |
|---|---|---|
| Architecture audit, foundation, job engine, security, recovery | GPT-5.6 Sol | Extra High; Pro for the hardest review when available |
| Merge vertical slice and cross-layer integration | GPT-5.6 Sol | High or Extra High |
| UI implementation and visual review | GPT-5.6 Sol | High |
| Repeating an established adapter pattern | GPT-5.6 Terra | High |
| Documentation, test cleanup, small mechanical changes | GPT-5.6 Terra or Luna | Medium |

Use the strongest model available in the Codex picker for milestone planning and risky changes. Do not use a large parallel-agent setup until the repository builds, tests, and has separate worktrees.

## Permissions and safety defaults

- Run locally or in a Git worktree.
- Keep approval prompts enabled.
- Internet access is off by default; enable it only for explicit package/release research or dependency downloads.
- Never grant arbitrary filesystem access beyond the project and approved test/output directories.
- Never allow two active goals to modify the same checkout or files.
- Pause before adding a new third-party engine, changing an architecture contract, weakening privacy, or performing destructive Git/filesystem actions.

## G00 — Readiness audit (`/plan`, no edits)

### Outcome

Determine whether the repository and Windows toolchain are ready for the first implementation goal.

### Plan prompt

```text
/plan

Audit Document Studio for implementation readiness without editing files, installing packages, committing, or pushing.

Read AGENTS.md, CODEX_START_HERE.md, FINAL_RECHECK.md,
docs/22-IMPLEMENTATION-READINESS-CHECKLIST.md,
docs/23-DEVELOPMENT-SETUP.md, docs/19-CODEX-DELIVERY-METHOD.md,
and codex/goals/README.md.

Inspect Git state, repository manifests, Node/npm, Rust/Cargo, Python, MSVC Build Tools, WebView2, Tauri prerequisites, package consistency, validation scripts, and current test/build feasibility.

Return:
1. READY, READY WITH FIXES, or BLOCKED.
2. Exact versions and commands executed.
3. Passed checks.
4. Blockers and exact remediation commands.
5. Proposed file-level plan for G01.
6. Risks requiring an ADR or human decision.

Do not modify the repository. Stop after the readiness plan.
```

### Exit gate

- Repository validators pass or blockers are documented.
- Git is initialized and clean.
- Required toolchain versions are known.
- G01 implementation plan is reviewable.

## G01 — Foundation goal

### Outcome

Produce a runnable and tested Phase 0 Windows application foundation with the Tauri/React shell, design tokens, SQLite-backed durable job lifecycle, secure per-job workspaces, `diagnostic.copy`, cancellation, verification, dependency diagnostics, and startup recovery.

### Goal prompt

```text
/goal

Bring Document Studio Phase 0 to a complete, runnable, testable state on Windows.

Outcome:
- Tauri/React application launches and follows the committed Precision Paper design tokens.
- SQLite migrations and repository layer persist jobs, inputs, steps, outputs, presets, workflows, dependencies and settings.
- The typed job state machine supports inspect, preflight, ready, running, verifying, publishing, completed, failed, cancelled and interrupted recovery states.
- Secure per-job workspaces, staged outputs, atomic publication where possible, cleanup, and startup reconciliation work.
- diagnostic.copy is implemented end-to-end with real progress, cancellation, verification, history and diagnostics.
- Dependency diagnostics and the Phase 0 acceptance screen are usable.

Constraints:
- Read and obey AGENTS.md, CODEX_START_HERE.md, relevant ADRs, contracts and design-system files.
- Preserve the approved Tauri + React + TypeScript + Rust + SQLite architecture.
- Keep processing local and network-free by default.
- Never overwrite input files.
- Use validated typed commands and argument arrays; no shell-string construction.
- Do not add qpdf, OCR, Office conversion, AI, public cloud, or unrelated product features in this goal.
- Pause for approval before adding a new dependency, changing a public contract/schema, or making an architectural decision not covered by an ADR.
- Make small commits/checkpoints and keep the working tree reviewable.

Verification and definition of done:
- python scripts/validate_repo.py passes.
- python scripts/check_links.py passes.
- npm install completes and the lockfile is committed.
- npm run typecheck and applicable frontend tests pass.
- cargo fmt --all -- --check, cargo clippy, and cargo test pass.
- A Tauri development smoke test launches successfully.
- Tests demonstrate diagnostic.copy success, cancellation, failure cleanup, path safety, no input overwrite, and forced-interruption recovery.
- UI keyboard navigation, visible focus, 200% zoom smoke, and high-contrast basics are checked.
- Exact commands, outputs, changed files, screenshots, known limitations and rollback notes are recorded under docs/implementation-log/.
- Stop only when the Phase 0 acceptance gate passes or a genuine blocker requires human input.
```

## G02 — PDF Merge vertical slice

### Outcome

Implement the first production document operation from file selection through verified output and history.

### Completion criteria

- Multiple PDFs can be added, inspected, reordered and optionally constrained by page ranges.
- Encrypted, unreadable, malformed and unsupported inputs fail in preflight with actionable messages.
- qpdf is invoked through a checksum-governed adapter and validated argument arrays.
- Progress and cancellation produce truthful terminal states and no published partial output.
- Output is reopened and verified for page count, source order, openability and expected encryption state.
- Naming collisions are handled without silent overwrite.
- Golden merge, encrypted/corrupt input, cancel, collision and crash-recovery tests pass.
- Performance baseline is recorded for the agreed corpus.

Use `codex/goals/G02-pdf-merge.md` as the `/goal` text.

## G03 — Core PDF and viewer

Implement PDF.js progressive rendering and virtualization, then reuse the verified qpdf pattern for split, extract, remove, reorder, rotate, linearize and PDF-to-images. Do not duplicate job architecture. Pass memory, scrolling, page-order, cancellation and golden-output gates.

## G04 — Optimize and convert

Implement lossless/balanced compression, image conversions, Office-to-PDF, text/Markdown/HTML-to-PDF, output naming and batch isolation. Keep Office workers in isolated profiles. Record fidelity and size/quality benchmarks.

## G05 — OCR and safety

Implement English/Hindi/Telugu searchable PDF, mixed-language OCR, preprocessing, protection/unlock, metadata controls, sanitization and true redaction. Redaction is complete only when underlying content is no longer recoverable by extraction or visual reconstruction under the defined mode.

## G06 — Workbench, forms and signing

Implement the persistent editing workbench, overlays, annotations, crop, forms, flattening, signature placement and cryptographic-signing spike. Coordinate-system decisions and certificate handling require ADRs and specialist review gates.

## G07 — Automation, optional AI and hardening

Implement saved workflows, watched folders, CLI, model manager and optional page-grounded document intelligence only after deterministic core milestones pass. Models remain explicit downloads with pinned revisions, licenses, hardware requirements and benchmark status. Finish with installer, updates, accessibility, performance and recovery hardening.

## Review cadence

At the end of each goal:

1. Ask Codex for `/status` and a concise completion report.
2. Run `/review` against the base branch.
3. Inspect the diff and test evidence.
4. Resolve failures in the same goal chat.
5. Commit or open a pull request only after the acceptance gate passes.
6. Update the Notion Build Execution Log and attach screenshots/test evidence when the Notion write connection is available.
7. Merge, then begin the next goal in a new worktree/chat.

## First action now

1. Place the repository at `C:\Dev\document-studio`.
2. Initialize and push the private Git repository.
3. Install the target toolchain.
4. Open Codex at the repository root.
5. Run the G00 `/plan` audit.
6. After review, start G01 using `/goal`.
