# Security Policy

## Reporting a vulnerability

Please **do not open a public issue** for security problems.

Report vulnerabilities privately through GitHub's Security Advisories:

1. Go to the repository's **Security** tab.
2. Click **Report a vulnerability**.
3. Fill in the advisory form with as much detail as you can.

Include the affected version, steps to reproduce, and the impact you observed.
You will normally get an acknowledgement within a few days. Fixes are coordinated
privately with the reporter before any public disclosure.

## Scope note: API keys are stored unencrypted

Octopus Chat stores provider API keys (both built-in and custom providers)
**unencrypted** in the local SQLite database at
`%APPDATA%/octopus-chat/octopus.db` (Linux: `~/.local/share/octopus-chat/octopus.db`,
macOS: `~/Library/Application Support/octopus-chat/octopus.db`).

This is deliberate for a local-first app, but it means **anything that can read
your user profile can read your keys**. Treat the database as a secret: do not
commit it, attach it to bug reports, or sync it to a public location. If you
rotate or revoke a key after sharing the database, treat the old key as
compromised.

## Out of scope

- Vulnerabilities in third-party dependencies with no demonstrated impact on
  this project. Report those upstream.
- Reports that require an attacker to already have local code execution as your
  user, absent a clear privilege-escalation path.