# Security Policy

## Scope and support status

Omarchy Quick Share is undergoing interoperability and security validation. No release is designated here as a guaranteed security-maintained version, and no audit or universal Android compatibility is claimed. Report against the current repository revision when possible; include the affected revision/version and component without exposing private data.

Relevant areas include key exchange/encryption, authentication/consent, malformed protocol inputs/resource limits, file-name/path handling, local D-Bus/session boundaries, temporary files, radio/network state restoration and installation ownership checks. Existing native empty-file and Android partial-cleanup limitations are recorded in [docs/VERIFICATION.md](docs/VERIFICATION.md); recording them does not waive acceptance criteria.

## Reporting a suspected vulnerability

**Do not post exploit details, private file contents, verification codes, credentials, keys, location metadata or device/network identifiers in a public issue or pull request.**

GitHub private vulnerability reporting is enabled for this repository. Use [Report a vulnerability](https://github.com/PuroDelphi/omarchy-quickshare/security/advisories/new) to contact the maintainer privately. The repository owner is [PuroDelphi](https://github.com/PuroDelphi). Do not substitute a public issue for a private report; no response SLA is promised.

In a private report, include the affected revision/component, impact, prerequisites, a minimal reproduction on your own devices, observed versus expected behavior, and any proposed fix. Use synthetic files and redact logs. Do not access another person's devices or data to demonstrate a finding.

## Safe investigation and disclosure

- Test only on equipment/networks you own or are authorized to use. Radio discovery and protocol traffic can affect nearby devices.
- Never automate code acceptance, disable security controls to call a test successful or disclose hotspot credentials.
- Prefer isolated local regression tests for malformed inputs. A Linux peer does not establish Android safety/interoperability.
- Preserve evidence of failures, including partial files, without distributing their contents or coordinates. Do not manually delete remote files and claim protocol cleanup.
- Coordinate disclosure and remediation with the owner before publishing technical exploit details. Publication timing depends on the actual issue and available coordination; this document promises no deadline.

Users should compare verification codes, use temporary visibility, reject unexpected requests, leave Wi-Fi-interruption consent off unless intended, and scope firewall/network permissions narrowly. Diagnostics may contain identifying data: redact before sharing.
