# Implementation Readiness Checklist

## Identity and scope

- [x] Canonical name is Document Studio.
- [x] Desktop-first platform strategy chosen.
- [x] Unified feature catalogue created.
- [x] Personal edition has no artificial plan split.

## Architecture

- [x] Tauri/Rust desktop boundary chosen.
- [x] Operation lifecycle and job state model defined.
- [x] Local SQLite and future cloud storage separated.
- [x] Browser/local/cloud routing documented.
- [x] Security/privacy and recovery rules documented.

## Design

- [x] Information architecture defined.
- [x] Screen specifications and core components defined.
- [x] Tokens and Figma handoff included.
- [x] Accessibility criteria included.
- [x] Prototype included.

## Engineering

- [x] Repository scaffold and AGENTS file included.
- [x] Codex prompts included.
- [x] Dependency/model registers included.
- [x] Validation scripts and CI skeleton included.
- [x] Create private GitHub repository and push the accepted baseline package.
- [ ] Create/import live Notion knowledge base.
- [ ] Create live Figma file from the handoff.
- [x] Run and verify the Windows Tauri toolchain on the target machine.
- [x] Complete the G01 Phase 0 interactive launch and screenshot evidence.
- [ ] Commit the approved G01 implementation and prove final clean Git status.

## Proceed/no-proceed

The G01 implementation, locked checks, interactive Windows smoke and screenshot evidence are complete. Production PDF operations remain blocked until the G01 commit and clean-status review are approved and completed. Notion and Figma publication are separate external activities and are not Phase 0 runtime dependencies.

## Final local recheck

The local package validation and publication boundary are recorded in `../FINAL_RECHECK.md`. The unchecked items above require connected external accounts or the target development machine; they are not silently marked complete.
