---
applyTo: ".agents/skills/**,.claude/skills/**,skills-lock.json,.devcontainer/devcontainer-lock.json"
excludeAgent: "coding-agent"
---

These third-party skills, symlinks, and lockfiles are maintained by the skills installer or
devcontainer tooling, not edited by hand. Do not comment on style, wording, correctness, or design,
and omit these files from the review summary unless reporting a clear security risk.

Still flag clear security risks introduced by the diff, such as exposed secrets, destructive
commands, unverified downloads executed as code, data exfiltration, or unexpected changes to a
skill's source in `skills-lock.json`.
