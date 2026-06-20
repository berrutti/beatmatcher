# Security Policy

## Supported Versions

Beatmatcher is a single-maintainer desktop app with no version branches.  
Only the latest release on the [Releases](https://github.com/berrutti/beatmatcher/releases) page is supported; please upgrade before reporting an issue.

## Reporting a Vulnerability

Please report security issues privately through GitHub's [Security Advisories](https://github.com/berrutti/beatmatcher/security/advisories/new) ("Report a vulnerability" under the repo's Security tab) rather than opening a public issue.

I'll acknowledge reports within a few days and aim to ship a fix in the next release. Response times can vary; I'll keep you updated on progress either way.

Beatmatcher is a local desktop app: it has no server component or user accounts, but it does load audio files from disk and run with the privileges Tauri grants the app (filesystem access, audio device access). Reports about malicious audio files, IPC/command injection, or anything that lets an untrusted file or input escalate beyond what the app is meant to do are in scope.
