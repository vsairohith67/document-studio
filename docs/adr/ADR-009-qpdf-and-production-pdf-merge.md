# ADR-009: qpdf and production PDF Merge

Status: Accepted; implemented in G02 and ready to stage

## Context

G02 is the first operation that processes real documents. It must merge 2–128 local PDFs in the exact user-visible order without rewriting the sources, silently repairing malformed files, overwriting an existing destination, or trusting a document engine's exit code as the only proof of success.

The G01 foundation already owns job metadata, private workspaces, cancellation, publication, cleanup and restart recovery. G02 extends those mechanisms; it does not replace them and does not add a database migration or Tauri capability.

## Decision

Document Studio will bundle qpdf 12.3.2 from the official `qpdf-12.3.2-msvc64.zip` release asset. The approved source archive SHA-256 is `8941870a604e7c87ed24566b038d46c24ce76616254d2383c578f60c0677f202`. The signed upstream checksum bundle must verify for identity `ejb@ql.org` and issuer `https://github.com/login/oauth` before any runtime file is accepted.

qpdf is Apache-2.0. The application bundle must retain its license, applicable notices, provenance files and an exact per-file runtime manifest. No PATH-installed qpdf, runtime download, updater, installer, registry change or fallback engine is allowed.

The operation contract is `pdf.merge` version `1.0.0`:

- Accept 2–128 regular local files identified as PDF by case-insensitive extension, `%PDF-` within the first 1,024 bytes and strict qpdf inspection.
- Preserve every persisted input ordinal, including deliberate duplicate entries. Expensive preflight inspection is deduplicated by Windows source-file identity with at most four unique sources processed concurrently, additionally bounded by available parallelism. This result sharing never changes the ordered input list.
- Create one physical short ASCII-named workspace snapshot for every persisted ordinal: `inputs/source-0000.pdf`, `inputs/source-0001.pdf`, and so on. Even ordinals that refer to the same source identity have distinct snapshot files and distinct qpdf arguments. UI order, database ordinals, snapshot names and `--file` arguments are one-to-one.
- Produce one page-only `application/pdf` output. Document metadata, bookmarks, attachments, interactive forms and signatures are unsupported and are not preserved as supported features. Existing digital signatures will not remain valid. G02 does not claim that every unsupported structure is physically removed from every output.
- Reject encrypted PDFs, zero-page PDFs, malformed PDFs and inputs for which qpdf reports that recovery would be required.
- Never modify or pass an original source path to qpdf.

The production merge argument vector is fixed as follows and does not include `--deterministic-id`:

```text
qpdf.exe
--empty
--suppress-recovery
--stream-data=preserve
--object-streams=preserve
--remove-info
--remove-metadata
--remove-page-labels
--pages
--file=inputs\source-0000.pdf
--file=inputs\source-0001.pdf
...
--
staging\merged.pdf
```

qpdf runs with the owned job workspace as its working directory, so every document argument is a short relative ASCII path. The implementation must compare the built vector with this contract and smoke-test it with qpdf 12.3.2 before the command is accepted. `--deterministic-id` is allowed only while generating deterministic test fixtures and is forbidden in the production merge builder.

The Rust process launcher will invoke the executable and an argument array directly. It must not call `cmd.exe`, PowerShell, a batch file, Tauri shell or a qpdf job JSON file. qpdf will run in the fixed production AppContainer profile `DocumentStudio.PdfEngine.Qpdf.V1` with zero capabilities and an owned Job Object with kill-on-close, one-process, 2 GiB process-memory and 30-minute wall-clock limits. Profile creation is idempotent: derive and verify the expected SID, verify the stored configuration version and empty capability set, and reuse only an exact match. An existing mismatch fails closed. Production never creates random or per-job profiles. Tests use only the fixed `DocumentStudio.PdfEngine.Qpdf.V1.Test` profile, validate its SID before use, and delete only that exact proven test profile during setup/teardown. A future uninstaller must remove the production profile only after deriving and matching its expected SID; G02 does not perform uninstall cleanup.

Only the verified engine cache, this job's snapshot inputs, staging and temporary directories are made available. The child receives `SystemRoot` and `WINDIR`; `TEMP`, `TMP`, and the Windows-required `LOCALAPPDATA` are set to the private job temporary directory rather than forwarded from the user profile. No proxy, cloud, real profile, `QPDF_*`, or general parent environment value is forwarded. Stdin is closed, stdout and stderr are continuously drained into bounded in-memory buffers, raw output is never persisted, and user-visible failures use allow-listed codes. If this sandbox cannot be proven on supported Windows, qpdf is unhealthy and PDF Merge is unavailable; there is no unsandboxed fallback.

After a successful merge exit, Rust will reopen the staging file and verify that it is an owned non-reparse regular file with plausible nonzero size and PDF magic. A separate sandboxed `--suppress-recovery --check` accepts exit 0 and rejects exits 2 or 3; every other outcome is a process/dependency failure. A separate `--is-encrypted` rejects exit 0 as encrypted, accepts exit 2 as unencrypted, and treats every other exit as a process/dependency failure. `--show-npages` must equal the checked sum of every ordered input, including duplicates. Rust records staging SHA-256 and size before publication, then reopens the published file and requires the final hash and size to match.

Cancellation remains cooperative during inspection, snapshotting and verification and terminates only the retained owned Job Object during qpdf execution. G01's atomic publication boundary decides cancellation-versus-commit. Startup does not resume a merge: it cleans only marker-proven workspace artifacts and identity-proven partials, reconciles matching publication evidence, and otherwise fails or remains interrupted without guessing ownership.

## Distribution and update policy

The reviewed acquisition script refuses an existing destination, downloads only the three approved official release assets, verifies Cosign provenance and the fixed archive hash, and creates a manifest for the selected executable, sibling runtime DLLs, license/notices and provenance. The complete Apache-2.0 text is copied from a repository-controlled file with a fixed SHA-256 because the upstream binary archive carries a license reference rather than the complete standard text. A separate verification script rejects missing, altered, unreviewed or unmanifested files and requires exact version output `qpdf version 12.3.2`.

Security and advisory review occurs monthly, general version review quarterly, and critical qpdf or bundled-crypto advisories trigger expedited review. Every version change needs a new explicit dependency approval, provenance record, runtime manifest, ADR update and complete regression run.

Bounded G02 measurements on the named Windows reference machine proved the small-merge, parent-memory, qpdf-memory, inspection, and cancellation budgets. The 1,000-page structural corpus was only 234,020 source bytes, so it does not prove the planned approximately 1 GiB byte-volume budget. That large byte-volume run and the 128-input full-preflight budget remain release-acceptance measurements; they do not block staging. Exact evidence is in `docs/08-PERFORMANCE-ARCHITECTURE.md` and the implementation log.

## Rejected alternatives

- pdfcpu remains self-described as alpha and would introduce another structural engine/configuration surface.
- MuPDF adds a rendering engine outside G02 and requires AGPL compliance or a commercial license for embedding.
- pikepdf still uses libqpdf while adding Python and wheel/runtime dependencies.
- lopdf's high-level in-memory model is a poorer fit for large or hostile inputs and diverges from the accepted structural-PDF architecture.

## Consequences

- The Cosign installation and qpdf acquisition/bundling gates passed on 17 August 2026. The production merge worker and UI are implemented on the G02 feature branch.
- The qpdf resource is 16 files and 8,574,799 bytes, including the manifest, runtime, licenses, and provenance.
- Merge progress during qpdf's opaque execution phase is indeterminate. The UI reports truthful stages and never invents a percentage.
- Redistribution must carry Apache-2.0 licensing and all applicable upstream/third-party notices. Installer signing and release packaging remain out of scope for G02.
