## Problem and change

Describe the consumer-visible problem and the proposed change.

## Verification

List commands/scenarios actually exercised, their results, and any checks not run. Distinguish deterministic tests, controlled UI fixtures, and native Android evidence. Reuse already approved physical cases unless this change affects that behavior.

## Compatibility and safety

- [ ] Consent, verification-code comparison, encryption and safe file publication remain intact.
- [ ] English and Spanish UI strings are updated together when affected.
- [ ] Installation changes preserve unrelated configuration, preferences and downloads.
- [ ] Documentation and any affected callers are updated.
- [ ] Imported code retains its license and provenance; vendored GPL code is not relicensed as MIT.
- [ ] No verification codes, keys, credentials, location data or private device/network identifiers are included.

Report suspected vulnerabilities privately through the Security tab, not in this pull request.
