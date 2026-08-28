# G04D-C non-mutating LibreOffice runtime admission proof

## Scope

This branch contains proof infrastructure only. It does not implement `office.to-pdf`, G04D1, G04D2, G04F2 or G05. It adds no production dependency, operation, contract, UI, migration, adapter or runtime bundle.

Starting accepted main: `a664dea4755122deefceac9d6ef699a920acc398`.

Proof branch: `feat/g04d-c-runtime-admission`.

## Validation rubric

The security/provenance review uses these five fail-closed criteria:

- [ ] The independently acquired MSI matches every frozen byte, signature, timestamp and Windows Installer identity value.
- [ ] The actual MSI feature/component graph proves the candidate ownership model, and dynamic pre/post evidence shows no protected host mutation.
- [ ] The realistic synthetic conversion interface works only with an isolated profile and inside the separate zero-capability AppContainer/owned Job Object boundary.
- [ ] qpdf and PDF.js independently reopen the exact staged PDF while source, profile, process and network controls remain intact.
- [ ] Exact owned cleanup succeeds; minimal MSI uninstall restores pre-state; no unrelated process or path is altered.

Unchecked boxes remain proof gaps until the exact branch-head manual workflow finishes. Local static checks cannot convert them to runtime evidence.

## Implemented proof surfaces

- `.github/workflows/g04d-c-libreoffice-runtime-proof.yml` has only `workflow_dispatch` and uses separate `windows-2025` jobs for administrative image and minimal MSI.
- `G04DC.Common.psm1` pins acquisition identity, exports/hashes the live MSI tables and both install/admin sequences, builds the feature/component/effect map, seals authoritative Windows Installer registration/cache state plus bounded host catalogs, and supplies typed fail-closed assertions.
- `Invoke-G04DCAdminImageProof.ps1` runs `msiexec /a`, verifies full-catalog non-mutation, manifests the extracted tree, runs version and conversion only inside the zero-capability AppContainer/Job boundary and cleans only its GUID marker-owned runner root.
- `Invoke-G04DCMinimalMsiProof.ps1` derives `INSTALLLEVEL=0`, ordered exact `ADDLOCAL`/`REMOVE`, and the public-property vector from the package. Native Windows Installer condition evaluation determines expected state for every selected conditioned component. Before installation, every mutating MSI-table row must map to a disabled component and every install custom action must be statically bounded; otherwise the mode seals `REJECTED` without invoking MSI. If that model closes, the proof requires exit 0 plus exact ARP and Windows Installer ProductState/cache/Products/Features/UpgradeCode/Components registration, exact installed feature/component/File-table state, protected non-mutation before and after smoke, an exit-0 exact uninstall without reboot, an absent install root before owned cleanup, and complete pre-state restoration.
- `G04DC.Sandbox.cs` is a proof-only launcher. It creates the separate zero-capability profile, launches suspended, assigns the Job Object before resume, records complete Job accounting plus resolved process/module identities and sampled TCP/UDP sockets, preserves structured failure evidence and deletes only that profile. The wrapper requires no loopback exemption and validates every dynamically loaded module against exact runtime/Windows roots and signer chains. It records ACL grants, bounded owned writable-root inventories, runtime/fixture immutability and package-storage/registry cleanup, but it truthfully rejects candidate admission because those observations do not empirically prove effective file access or outside-root write denial.
- `Invoke-G04DCLocalMsiReadOnlyValidation.ps1` can reproduce table/condition/effect analysis against the already accepted local MSI without installing or launching LibreOffice.
- `New-G04DCSyntheticOdt.ps1` creates the tiny licence-clean fixture. `verify-pdfjs.mjs` confirms the generated PDF opens and contains only the expected canary assertion.
- `Test-G04DCBoundaries.ps1` covers the requested static failure classes without installing or launching LibreOffice locally.

## Current evidence state

The local proof-only validation completed without launching or installing LibreOffice:

- Windows PowerShell 5.1 parses all retained proof scripts/modules, and its compiler accepts both proof-only C# helpers: the AppContainer/Job launcher and native MSI condition evaluator.
- The fail-closed harness defines 53 named cases, including signer-chain, cross-origin/nonstandard-port acquisition, conditioned-component, MSI effect/custom-action closure, authoritative Installer registration/cache, sibling-prefix, unresolved and missed short-lived descendant, dynamic-module root/signature, feature/file-ownership, unrelated-product, full classes/environment/shortcut/Services/Installer-cache catalogs, exact marker-owned cleanup, artifact-manifest, file-access, and full service-catalog failures.
- Read-only inspection of the exact accepted local MSI derived a 23-feature, 581-selected-component Writer/Calc/Impress closure with no feature/component ownership ambiguity and no selected Font-table component. The marker-owned evidence is `C:\Dev\document-studio-worktrees\_resources\g04d-c-runtime-admission\local-msi-readonly-validation-9`; its artifact-manifest SHA-256 is `44704b9eceb165e291d83754b3166b2d0bbaad5c4ec6d0eb1eed044ccb5ec344`.
- Exact Directory/Component/File analysis found 29 selected VC runtime components outside `INSTALLLOCATION`; native condition evaluation under derived `VC_REDIST=0` must prove each absent, while the exact 29 system targets remain unchanged. All other selected conditions are evaluated under the same candidate property vector.
- Every relevant standard MSI mutation table and both administrative sequences are exported. The exact package produced 1,937 mapped mutation rows, zero ambiguous rows, 25 enabled protected rows (20 `Registry`, five `Shortcut`), 34 unbounded install/UI custom-action sequence entries and zero unbounded administrative custom actions. The minimal MSI model therefore fails closed before installation; these static blockers are not waived in favor of dynamic measurement. Dynamic state sealing remains required for any mode whose static model closes.
- Current AppContainer evidence does not include empirical effective file-access telemetry. The smoke therefore fails closed with `FILE_ACCESS_BOUNDARY_INVALID`; inventories and ACLs are never reported as a passing substitute.
- `scripts/validate_repo.py`, `scripts/check_links.py`, `git diff --check`, Node syntax validation and YAML structure validation passed.
- Every pinned GitHub action commit resolved through the upstream GitHub repository API.

Runtime candidate classification, exact workflow head/run URLs and artifact hashes must come only from the manual exact-head GitHub Actions run.

Preliminary local source-boundary observation on 2026-08-28: the exact owner-specified `download.documentfoundation.org` URL returned an HTTP 302 to a MirrorBrain-selected mirror origin. Because mirrors are explicitly prohibited, the proof now records the redirect chain and fails provenance before downloading bytes. This local observation is not exact-head CI evidence and is not `LIBREOFFICE_RUNTIME_UNSUPPORTED`; the manual run must seal the then-current response.

The workflow result is an architecture-gate input, not G04D-B2 acceptance or production support.

## Explicit non-actions

- No ordinary full MSI install occurs on the laptop.
- No user document or ordinary LibreOffice profile is accessed.
- No manual font, service, association, UserChoice, registry or broad filesystem cleanup occurs.
- No production code, merge, tag, package, release, deployment or tracker update is included.
