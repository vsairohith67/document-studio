# Third-Party Notices and Adoption Gate

This repository does not bundle production binaries or model weights. Candidate dependencies and models are documented for evaluation; their licenses, signatures, checksums, distribution terms and security posture must be rechecked before each release.

The authoritative register is `docs/14-DEPENDENCY-AND-LICENSE-REGISTER.md`. Model-specific governance is in `docs/15-HUGGING-FACE-MODEL-PLAN.md` and `models/models.yaml`.

A dependency may move from evaluation to adoption only after an ADR records:

- exact version or immutable revision;
- source and binary provenance;
- license and redistribution obligations;
- vulnerability/update policy;
- sandboxing and failure behavior;
- benchmark and output-verification evidence.
