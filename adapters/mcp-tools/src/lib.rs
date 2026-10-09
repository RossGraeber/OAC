// SPDX-License-Identifier: Apache-2.0

//! The OAC MCP tool surface: `send`, `reply`, `list_sessions` and `whoami`
//! (`spec/bindings/mcp.md` §5), shared by both provider adapters so that every harness sees
//! the same tool names, `inputSchema` and result shape ([MCPB-TOOL-003]). Adapters may not
//! depend on each other (07 §3), so the one definition lives here.
//!
//! Skeleton only (#7, `docs/planning/decisions/G-7-stage4-dependencies.md` §4): no tool
//! logic yet. G5 (#66) and G8 (#69) fill it. It depends on `oac-core` only, holds no
//! provider-specific type, and invokes no transport operation ([IFC-ADP-001]).
