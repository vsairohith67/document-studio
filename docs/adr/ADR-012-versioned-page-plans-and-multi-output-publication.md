# ADR-012: Versioned page plans and truthful multi-output publication

Status: Accepted; implemented in G03

## Context

Reorder, remove, extract, rotate and split need durable intent that can survive a process failure. The accepted schema did not store a typed page plan or multiple expected outputs.

## Decision

Migration 4 adds `job_operation_plans`, keyed one-to-one by job ID. Its canonical JSON envelope contains `schemaVersion`, `operationId`, `sourcePageCount` and typed `payload`. The database requires `length(CAST(plan_json AS BLOB)) BETWEEN 2 AND 65536`; Rust independently enforces the exact UTF-8 byte limit, canonical serialization and SHA-256. It stores no document bytes, text, thumbnails or passwords. UI numbers are 1-based; every persisted page index is 0-based.

Each operation is version `1.0.0`. A fresh sandboxed qpdf page count must match `sourcePageCount` before execution. Extract uses a nonempty unique page list in selected order. Remove computes the ordered complement and leaves at least one page. Reorder requires an exact permutation. Rotate requires unique selected pages and 90, 180 or 270 clockwise degrees after existing rotation is flattened. Split requires 1–128 uniquely named, ordered, non-overlapping ranges that partition the whole source.

All expected `job_outputs` rows are inserted before processing. Each staging PDF is independently checked as an owned regular file with PDF magic, nonzero size, SHA-256, strict qpdf structure, unencrypted state, exact page count and expected rotation where applicable. Runtime verification does not claim arbitrary visual page identity; adversarial deterministic fixtures prove operation order semantics.

Publication is sequential and truthful, not falsely atomic. A job is completed only when every expected output is published and final size/hash equal staging evidence. If a later output fails, the job becomes failed with `PARTIAL_PUBLICATION`; already published user files remain recorded and are never deleted. Restart recovery never resumes qpdf, preserves published files, and cleans only proven-owned private/unpublished artifacts.

## Compatibility and rollback

Migrations are append-only and checksum-verified. Existing G01/G02 rows need no plan and keep their behavior. Code that does not understand migration 4 must not open the upgraded database. Rolling back means restoring a pre-migration database backup with older code; dropping the table in place is not an approved rollback.

## Alternatives rejected

- Encoding page intent in UI state or log text is not durable or typed.
- One organizer operation ID would hide distinct semantics and recovery rules.
- All-or-nothing claims across ordinary user-directory file publications are untrue without a new filesystem transaction boundary.
