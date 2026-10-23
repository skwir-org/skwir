# ADR-085: Agent and script connections have separate authority

Status: accepted, 2026-10-23. Amends ADR-043, decision 5.

## Context

ADR-043 assumed an agent would not fill MCP metadata. An MCP caller can
send arbitrary metadata, so `skwir/plan: apply` bypassed ADR-044's review,
permission checks and privacy filtering. The request cannot establish
its own authority.

## Decision

- The control plugin binds two endpoints with the same operating-system
  access restrictions and lifetime. Its existing address serves agents;
  the address with `.scripts` appended serves the user's scripts.
- `skwir mcp` forwards only to the agent endpoint. Every call accepted
  there runs in Propose mode, whatever its metadata says.
- `skwir cmd` connects to the script endpoint for Print or Apply. Only
  that listener accepts `skwir/plan` as a choice of execution mode.
- Both listeners must bind before the plugin reports that it listens.
  Failure or disable removes both. The core command surface is unchanged.

## Consequences

An agent with only the MCP connection cannot promote a request to a
user's action. As before, this is not a sandbox for arbitrary programs
running as the user: such a program can invoke `skwir cmd` itself or use
the script endpoint. Operating-system isolation governs that access.

Existing script clients need the new endpoint; restart an older window
before using the updated CLI. Tests send forged metadata over the actual
agent transport and check review, private answers and user-only commands;
the existing script tests still exercise direct print and apply.
