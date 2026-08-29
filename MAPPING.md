# MAPPING.md — Map of Crates to Official Concepts

This document maps the crates in the `ai-bridge` workspace to the official
architectural concepts they serve, based on the project documentation.

## Workspace crates

| Crate | Official concept served |
| ----- | ----------------------- |
| `ai-bridge-channels` | OS/socket-level identity enforcement via the three Unix domain sockets (`private_a.sock`, `private_b.sock`, `public_maestro.sock`) and `SO_PEERCRED` peer-PID validation. |
| `ai-bridge-protocol` | The strict JSON message schemas exchanged across the bridge: `SubChatOpenRequest`, `ContextQuery`, `ContextResponse`, `SubChatResult`, `ExecutionPlan`. Encodes the context-exchange protocol. |
| `ai-bridge-gatekeeper` | The policy engine that classifies executions into `Delegable` / `NonDelegable` / `PermanentSuspension` across the six risk criteria, plus the 120-second timeout policy. |
| `ai-bridge-hand-eye` | Visual access controls (approved read extensions) and portal automation restrictions, driven by `allowlist.toml`. |
| `ai-bridge-subchat` | The multi-branch orchestration layer: `Branch` management and the context-exchange flow (`ContextQuery` → `ContextResponse` → `SubChatResult` with `call_index`) and `OrphanWorker` execution. |
| `ai-bridge-selfheal` | Automated self-healing: fault detection and ops-room escalation for reconnecting or recovering aborted operations. |

## Cross-cutting concepts

- **Identity is physical, not in-band.** Peers are authorized by process PID at
  the socket layer; application payloads never carry `sender` or `auth_token`.
  This is realized by `ai-bridge-channels` and guaranteed by `ai-bridge-protocol`.
- **Constrained delegation.** Executions are only delegated to sub-chats when
  the `ai-bridge-gatekeeper` marks them `Delegable`; `ai-bridge-subchat` then
  executes them through the context-exchange protocol.
