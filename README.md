# AI Bridge

AI Bridge is a Rust workspace that implements a constrained orchestration and safety pipeline for multi-branch execution, secure channel communication, policy enforcement, and automated self-healing. The design intentionally keeps identity enforcement at the OS/socket layer and prevents identity-bearing fields from appearing in application payloads.

## Architecture overview

### 1. Sockets and physical identity enforcement
The `ai-bridge-channels` crate manages three Unix domain sockets:

- `private_a.sock`
- `private_b.sock`
- `public_maestro.sock`

All socket accept paths use Linux `SO_PEERCRED` via the `nix` crate to extract the peer process credentials. The bound socket compares the connected process PID against the legally registered PID during bridge initialization. If the peer PID does not match, the connection is rejected immediately.

Application-level payloads intentionally do not carry `sender` or `auth_token` fields. Identity is treated as a physical property of the connection, not a message field.

### 2. Protocol layer
The `ai-bridge-protocol` crate defines the structured message schemas used for:

- `SubChatOpenRequest`
- `ContextQuery`
- `ContextResponse`
- `SubChatResult`
- `ExecutionPlan`

These are strict JSON-serializable structs with no identity-bearing fields.

### 3. Gatekeeper policy engine
The `ai-bridge-gatekeeper` crate applies policy enforcement across six criteria:

1. Irreversibility
2. System security
3. Credentials / secrets
4. Data exfiltration
5. Financial risk
6. Third-party impact

Any criterion hit makes the execution `NonDelegable`. Zero hits makes it `Delegable`. System wipe patterns are hard-blocked and produce `PermanentSuspension`.

A 120-second timeout is enforced:

- Delegable + timeout => Auto-Approve
- NonDelegable + timeout => Permanent Suspension

### 4. Hand-Eye and portal restrictions
The `ai-bridge-hand-eye` crate controls visual access and portal automation:

- Reads are limited to approved file extensions: `.png`, `.jpg`, `.jpeg`, `.webp`, `.pdf`
- Any other file type fails with a hard error
- The portal abstraction checks the loaded allowlist before any DBus call is made

### 5. Subchat coordination
The `ai-bridge-subchat` crate manages Branch A and Branch B execution flows and enforces the maximum of two sub-chat calls per task. Orphan workers (`new_aX`) are single-shot and cannot request further sub-chats.

### 6. Self-heal / Ops Room
The `ai-bridge-selfheal` crate isolates operational recovery flow from the socket and gatekeeper security layers. It handles detection of:

- explicit errors from the automation/connection layer
- silent timeout conditions

Then it emits operational directives for role swapping and continued execution on a healthy branch without executing system-level commands.

### 7. Root orchestration
The root executable coordinates both branch workers and implements the required 240-second synchronization timeout.

- If both branches finish before timeout: full delivery to Maestro
- If only one branch finishes and the timeout expires: partial delivery to Maestro
- On any delivery, both branch states are wiped by dropping the in-memory state to allow Rust to release them naturally

## Workspace structure

```text
.
├── Cargo.toml
├── README.md
├── config/
│   └── allowlist.toml
├── src/
│   ├── config.rs
│   ├── lib.rs
│   ├── main.rs
│   └── root_branch.rs
├── crates/
│   ├── ai-bridge-channels/
│   ├── ai-bridge-protocol/
│   ├── ai-bridge-gatekeeper/
│   ├── ai-bridge-hand-eye/
│   ├── ai-bridge-subchat/
│   └── ai-bridge-selfheal/
```

## Required allowlist file

The allowlist file is expected at:

```text
config/allowlist.toml
```

The file must use a TOML structure like this:

```toml
[[apps]]
app_id = "org.mozilla.firefox"
allowed = true

[[apps]]
app_id = "org.gnome.Nautilus"
allowed = true

[[apps]]
app_id = "org.gnome.Terminal"
allowed = false
```

The `ai-bridge-hand-eye` allowlist parser reads this file dynamically. Do not hardcode app names into Rust source.

## Building the project

From the workspace root:

```bash
cargo build --release
```

This produces the release binary in the standard Cargo target output location:

```text
target/release/ai-bridge
```

## Running the compiled binary

From the workspace root:

```bash
./target/release/ai-bridge
```

The binary initializes its runtime sockets, loads the allowlist, and begins the main orchestration loop.

## Manual socket validation with `SO_PEERCRED`

The security model is enforced physically at the socket layer. You can manually test the validation behavior using `socat` or `nc` to confirm that a process with an unauthorized PID is rejected.

### Example using `socat`

1. Start the bridge in one terminal session:

```bash
./target/release/ai-bridge
```

2. In another terminal, create a Unix domain socket client with `socat`:

```bash
socat - UNIX-CONNECT:/tmp/ai_bridge_runtime_sockets/public_maestro.sock
```

3. If the socket peer PID is not a registered and authorized bridge PID, the server will reject the connection.

### Example using `nc`

```bash
nc -U /tmp/ai_bridge_runtime_sockets/public_maestro.sock
```

If the connection is unauthorized, the server-side accept path should fail the PID check before handling the message. Successful connections are only accepted from the valid registered process.

### Verifying rejection behavior

To demonstrate unauthorized PID rejection explicitly, run a client from a different process identity or from a shell that is not the expected bridge process. The accept logic compares the peer PID returned by `SO_PEERCRED` against the registered bridge PID and will immediately reject mismatches.

A minimal shell example for a manual test is:

```bash
bash -c 'echo "hello" | nc -U /tmp/ai_bridge_runtime_sockets/public_maestro.sock'
```

The server should reject it unless the connecting process matches the legal registered PID.

## Notes

- The socket layer is the source of truth for identity enforcement.
- Message payloads must not contain identity fields.
- The gatekeeper must be passed any task through policy evaluation before task approval.
- The allowlist is loaded at runtime from `config/allowlist.toml`.
- The self-heal logic is operational-only and does not execute system-level commands.

## License

This project is delivered as a workspace implementation for the AI Bridge architecture and should be treated as a controlled engineering artifact.
