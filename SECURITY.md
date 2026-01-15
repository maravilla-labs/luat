# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue, please report it responsibly.

### How to Report

1. **Do not** open a public GitHub issue for security vulnerabilities
2. Email us at [security@maravilla-labs.com](mailto:security@maravilla-labs.com) with:
   - A description of the vulnerability
   - Steps to reproduce the issue
   - Potential impact assessment
   - Any suggested fixes (optional)

### What to Expect

- **Initial Response**: Within 48 hours of your report
- **Status Update**: Within 7 days with our assessment
- **Resolution Timeline**: We aim to address critical vulnerabilities within 30 days

### Disclosure Policy

- We will coordinate with you on disclosure timing
- We appreciate responsible disclosure and will credit reporters (unless anonymity is preferred)
- Please allow us reasonable time to address the issue before public disclosure

## Security Best Practices

When using luat:

- Always download releases from official sources (GitHub Releases, npm)
- Verify checksums when available
- Keep your installation updated to the latest version
- Review Lua scripts before execution, especially from untrusted sources

## Scope

This security policy applies to:

- The luat CLI tool
- Official npm packages (`@anthropic-ai/luat`, platform-specific packages)
- Code in this repository

Third-party dependencies are subject to their own security policies.
