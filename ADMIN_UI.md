# AI-Bridge Admin UI & Configuration Guide

This document explains every panel in the desktop **Admin** tab of the
`ai-bridge-ui` application, the underlying config files it manages, and the
**non-negotiable safety constraints** that the UI enforces.

The Admin UI is a **configuration surface**, not a bypass. It lets an authorized
operator view and edit the tunable parts of the system without touching source
code, while keeping the security-critical boundaries in compiled, reviewed Rust.

---

## Config file architecture

Each editable area maps to its own TOML file under the config directory
(`~/.config/ai-bridge/` on Linux/macOS, `%APPDATA%\ai-bridge\` on Windows).
Separate files are easier to version, diff, back up, and roll back individually.

| Panel | File | Contents |
|-------|------|----------|
| Executor Allowlist | `executor_allowlist.toml` | Allowed binaries/commands |
| Symbols & Policy | `symbol_policy.toml` | Symbol classification + auto-approve timing |
| Criteria Patterns | `criteria.toml` | Safety keyword/regex trigger lists |
| Sockets | `sockets.toml` | Socket paths + permission bits |
| Branches | `branches.toml` | Sub-chat limits + context merging |
| Audit Log | `audit/audit-YYYY-MM.jsonl` | Append-only change log |

The shipped files in the repo (`config/`) act as defaults; the installer seeds
them **only if absent**, so an upgrade never clobbers operator config.

---

## Panels

### 1. Executor Allowlist

Lists the binaries that the gatekeeper daemon may execute. Each row shows the
binary name, an optional resolved path, an `allowed` toggle, and optional
argument restrictions.

- Add / edit / remove entries.
- **Validation on save**: no duplicate names, and the wildcard path `*` is
  rejected as unsafe.
- Changes here require an **Apply & Restart Daemon** to take effect.

> The allowlist is an *independent* layer from Gatekeeper classification: even a
> `Delegable`-classified command still must name a listed binary to run.

### 2. Symbol & Policy Editor

Lets you edit, **for existing symbols only**, the Delegable/NonDelegable
classification mapping and the auto-approval timing window (seconds) for
Delegable tasks.

- The `Symbol` enum remains compiled Rust — the GUI **cannot create arbitrary
  new symbols**. Removed rows cannot be dropped.
- **CRITICAL symbols can never be reclassified as Delegable**, and their
  auto-approve window is locked at `0` (they never silently auto-approve).
  Any attempt is rejected by validation and flagged.
- The shipped default values match the historical hardcoded behavior.

### 3. Safety Criteria Pattern Editor

Exposes the keyword/regex trigger lists used by `criteria.rs`:

- Irreversibility (deletion / wipe)
- System security / permissions
- Credentials & secrets
- Data exfiltration
- Budget / financial impact
- External-party / third-party impact

The operator can add or remove trigger patterns per category.

- Includes a **live "test a sample command"** tool that evaluates an arbitrary
  command against the current in-memory criteria and reports DELEGABLE /
  NON-DELEGABLE plus which criteria triggered — without saving anything.
- Patterns are validated as regex before they can be saved.

### 4. Channels & Sockets Manager

Shows the active Unix Domain Socket definitions (paths and permission bits).
You can create/remove socket definitions and adjust permission bits.

- **Identity verification cannot be edited or bypassed here.** The PID→socket
  binding check (`SO_PEERCRED`) lives in `ai-bridge-channels` compiled code, not
  config. The GUI never moves identity into the message payload.
- Socket changes require a daemon restart (sockets are bound at startup).
- Removing a socket is flagged as a security-lowering change requiring
  confirmation.

### 5. Sub-chat / Branch Configuration

View and adjust Branch A / Branch B per-task sub-chat limits and whether master
context is merged by default. The `call_index` is derived from the sub-chat
counter and cannot be overridden to be non-monotonic.

### 6. Execution Log & Audit Trail

Read-only view of past changes:

- Every config change made through the GUI is logged: timestamp, operator,
  changed file, action, old value → new value, and whether the change was
  flagged security-lowering.
- Stored as **append-only JSONL** in `audit/audit-YYYY-MM.jsonl` (rotated
  monthly). Entries are immutable — never edited or deleted.
- The panel can also surface past `TaskState` execution records (command,
  symbol, policy decision, approval timing, stdout/stderr/exit code) as those
  are routed to the same audit store.

---

## Non-negotiable safety constraints

The GUI is a configuration surface, **never** a bypass:

1. **Shell-free execution is always enforced.** Commands still go through
   `tokio::process::Command` via `executor.rs` — there is no path that enables
   `bash -c`. The Admin UI has no control over this; it's compiled code.
2. **Identity/auth never moves into the message payload.** `SO_PEERCRED`
   identity stays at the socket/kernel layer (`ai-bridge-channels`). The GUI has
   no way to edit or bypass it.
3. **Security-lowering changes require explicit confirmation.** Any change that
   lowers security posture (reclassifying a Critical symbol as Delegable,
   enabling auto-approval on a Critical symbol, adding a wildcard allowlist
   entry, removing a socket) pops a confirmation dialog, is clearly flagged, and
   is recorded in the audit log with `security_lowering = true`.
4. **Validation before every write.** New config is validated (duplicates,
   unsafe wildcards, regex validity, octal permissions, critical-symbol rules)
   before being written. A malformed config is never silently applied.
5. **No silent hot-reload.** There is no live IPC reload endpoint. Changes are
   written to disk, validated, and applied via an explicit **"Apply & Restart
   Daemon"** action. Sockets (bound at startup) and policy (loaded at startup)
   pick up changes only after restart.
6. **Backup before write.** The prior config file is backed up
   (`*.bak.<timestamp>`) before any write, so operators can restore the
   previous state.

---

## "Apply & Restart Daemon" workflow

1. Edit the relevant panel.
2. Click **Save** — the change is validated (dry-run) and written to disk with a
   backup.
3. The change is append-logged to the audit trail.
4. Click **Apply & Restart Daemon** (or restart the daemon service) to load the
   new configuration.

Until restart, the running daemon retains the previous validated policy state —
a malformed or half-applied change can never corrupt a running policy engine.

---

## Installers

See [`packaging/README.md`](packaging/README.md) for the full installer story.
All three binaries (daemon, core, UI) ship in one package. Install into a
dedicated app directory (e.g. `/opt/ai-bridge/`), seed (never overwrite) the
user config directory, and make systemd/launch-agent/service setup **opt-in**.
Code signing is required before public distribution.
