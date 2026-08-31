# Config

Project configuration discovery, TOML parsing, workspace trust verification,
and configuration tracking.

## Language

### Configuration Files

#### Config File

A TOML configuration file at the local (`.traces/config.toml`) or global
(`~/.config/traces/config.toml`) level defining template, schema, and metadata
settings.
*Avoid*: settings file, preferences

#### Configuration Scope

The two-tier hierarchy where global defaults are overridden by local project
configuration.
*Avoid*: layered config, cascaded settings

### Trust & Security

#### Trust Verification

The security check performed before loading local configuration or executing
templates, rejecting untrusted or altered project roots.
*Avoid*: authorization check, security scan

#### Trust Status

The trust state of a workspace root: `Trusted` (known root with matching
digest), `Untrusted` (unknown root), or `Stale` (configuration modified since
trust was granted).
*Avoid*: trust level, security state

#### Companion Hash

The stored cryptographic digest of a `.traces/config.toml` file used to detect
out-of-band configuration changes.
*Avoid*: baseline hash, signature, checksum

#### Tracked Config Store

The user-level record of all local `.traces/config.toml` paths discovered during
execution, kept independent of trust decisions.
*Avoid*: config history, audit store

### Canonical Metadata

#### Metadata Roles

Canonical frontmatter keys configured under `[frontmatter]` mapping
project-specific names for title, aliases, creation date, and modification
date.
*Avoid*: field mapping, canonical attributes
