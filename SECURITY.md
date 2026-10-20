# Security Policy

## Reporting a vulnerability

Report vulnerabilities to the maintainer through a private channel. If GitHub offers private vulnerability reporting for this repository, use that option. If no private channel is available, ask the maintainer to arrange one before sharing details. Keep exploit details, credentials, and personal data out of public issues.

Include the version of `benitoiteh` and the Benitoite processor, the operating system, the mode and sandbox settings, and the smallest input or command that reproduces the problem. Describe the expected result and the actual result. Remove API keys, tokens, passwords, personal paths, and private session data from examples and logs.

## Security boundaries

BenitoiteHarness is a prototype. Its intended security boundaries and known limits are described in the [security model](docs/design/03-safety/03-01-security-model.md), [sandbox design](docs/design/03-safety/03-02-sandbox.md), and [approval rules](docs/design/03-safety/03-03-approval.md) (in Japanese).

Reports should identify the boundary affected, such as untrusted model output bypassing approval, a sandboxed process accessing a denied location, restricted mode starting an interpreter other than Benitoite, or credentials appearing in model input or logs. A script passing the Benitoite type and effect checks alone does not establish that its operations are authorized by the user.
