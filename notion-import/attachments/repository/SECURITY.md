# Security Policy

Document Studio processes untrusted documents and therefore treats parsers, converters, OCR engines and optional AI providers as security boundaries.

## Reporting

Keep security reports private until a fix is available. For this private repository, open a confidential GitHub security advisory or contact the repository owner directly. Do not attach real sensitive documents; use a minimal synthetic reproducer.

## Required handling

- Record affected version, operating system, operation, input characteristics and reproduction steps.
- Remove document contents, passwords, certificate material, API keys and personal information from reports.
- Preserve hashes and dependency versions when provenance is relevant.
- Do not publicly disclose a vulnerability before the owner has assessed and remediated it.

## Supported scope

The current package is a planning and Phase 0 starter, not a production release. Once releases begin, this file must list supported versions and response targets. Threat-model requirements live in `docs/13-SECURITY-PRIVACY-THREAT-MODEL.md`.
