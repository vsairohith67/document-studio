# ADR-004: Output verification is mandatory

Status: Accepted

A zero exit code is insufficient. Every operation defines invariants, reopens the staged output, verifies them and publishes only on success.
