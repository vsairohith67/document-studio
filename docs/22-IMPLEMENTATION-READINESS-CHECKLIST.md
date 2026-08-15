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
- [ ] Create private GitHub repository and push this package.
- [ ] Create/import live Notion knowledge base.
- [ ] Create live Figma file from the handoff.
- [ ] Run toolchain installation on the target Windows machine.
- [ ] Execute Phase 0 prompt in Codex.

## Proceed/no-proceed

Proceed to Phase 0 only after the five unchecked environment/publication items are confirmed. The planning package itself is complete enough to start.

## Final local recheck

The local package validation and publication boundary are recorded in `../FINAL_RECHECK.md`. The unchecked items above require connected external accounts or the target development machine; they are not silently marked complete.
