# Security policy

## Reporting a vulnerability

Please report security issues privately through [GitHub's private vulnerability reporting](https://github.com/cpreston321/encore/security/advisories/new), not in a public issue. Include the steps to reproduce the problem and what an attacker could achieve. You can expect a reply within a week.

## Scope

These parts of Encore handle credentials or accept connections, so they matter most:

- **The sign-in server** (`src/auth.rs`, `src/localhost.rs`). It runs on `127.0.0.1` while you sign in, and it accepts a Music User Token only with a random per-session value.
- **The web player bridge** (`src/webplayer.rs`). This is a localhost endpoint the hidden MusicKit page polls, and it's keyed with a random token.
- **Token storage.** The Music User Token is kept in the macOS Keychain. The MusicKit developer key never leaves your Mac.

Only the latest commit on `main` is supported.
