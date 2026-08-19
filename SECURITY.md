# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in markharness, please report it privately rather than opening a public issue.

Use [GitHub Security Advisories](https://github.com/markharness/markharness/security/advisories/new) to open a private report. This is the project's only monitored private reporting channel; please do not send vulnerability details to a personal email address or post them in a public issue.

Please include:

- A description of the vulnerability and its potential impact
- Steps to reproduce (a minimal `.markharness/knowledge/`/CLI invocation is ideal, since markharness only reads/writes local Git repository state)
- Any known mitigations

## Response

markharness is maintained by one person in their spare time. Reports are reviewed on a best-effort basis as capacity permits. There is no service-level agreement, and acknowledgement, investigation, remediation, disclosure, or a patch release cannot be guaranteed within any particular timeframe.

When a report is validated and a fix is practical, the maintainer will aim to coordinate disclosure through a GitHub Security Advisory and publish a patch release. Credit will be given to the reporter unless anonymity is requested.

## Supported Versions

No released version carries guaranteed security support. When a security fix is practical, it will normally target the latest release; older releases are not maintained. Users are responsible for evaluating project suitability and updating to an available fixed release.
