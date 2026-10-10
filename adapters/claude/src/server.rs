// SPDX-License-Identifier: Apache-2.0

//! The channel-path MCP server for one connection, on `rmcp` (G-7 §3.1; 11-risks row 16).
//!
//! It is the legacy-only exception of `spec/bindings/mcp.md` [MCPB-CLD-003]: a stdio server
//! started for the Claude channel path.
//!
//! - **`server/discover` gets `-32601`.** [`ChannelTransport`] answers the probe with
//!   `Method not found` before `rmcp` sees it ([MCPB-CLD-003]), so Claude Code falls back to
//!   `initialize`, as G4 and D6 recorded. `rmcp` `3.4.0` cannot do this itself (unit test
//!   `rmcp_alone_cannot_serve_the_probe_then_legacy_opening`): with legacy revisions only,
//!   it answers the probe with `-32022`, a recognized modern error; with the modern revision
//!   too, a connection opened by any request but `initialize` stays on the stateless era,
//!   and the legacy `tools/list` after the fallback is refused with `-32602` for want of
//!   per-request `_meta`. The handler's own `discover` answers `-32601` too.
//! - **Legacy only.** It supports the legacy revisions only, answers `initialize` at
//!   `2025-11-25` unless the client names another legacy revision it supports
//!   ([MCPB-ERA-001] to [MCPB-ERA-003]), and so declares `experimental["claude/channel"]`
//!   only in a legacy `initialize` result ([MCPB-CLD-001]).
//! - **The extension identifier** goes in `capabilities.extensions` of the `initialize`
//!   result ([MCPB-ERA-008]). Whether live Claude Code still registers the channel with it
//!   present is the binding's open hook H13, for the opt-in live leg (#65).
//! - **No resources, prompts or subscriptions**, and no logging: nothing a session could
//!   read repeatedly for messages ([MCPB-DLV-002]). Only the four tools of
//!   `oac-mcp-tools`.
//!
//! The attachment opens when the first `tools/list` answer has been written: Claude Code
//! takes channel notifications only once listing is done (D6 lines 3-10), so nothing is
//! handed off before then.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::Arc;

use oac_core::adapter::Attachment;
use oac_core::delivery::ErrorCode;
use oac_core::ids::EXTENSION_ID_V0;
use oac_mcp_tools::CallError;
use rmcp::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ClientJsonRpcMessage, ClientRequest,
    DiscoverRequestMethod, DiscoverResult, ErrorData, Implementation, JsonObject, JsonRpcMessage,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerConfig,
    ServerJsonRpcMessage, ServerResult, ToolsCapability,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::Transport;
use rmcp::transport::async_rw::AsyncRwTransport;

use crate::adapter::Inner;
use crate::channel::{CHANNEL_CAPABILITY, INSTRUCTIONS};
use crate::io::{ChannelReader, ChannelWriter};

/// The legacy revisions this server speaks, newest first ([MCPB-ERA-002]).
pub const LEGACY_REVISIONS: [ProtocolVersion; 4] = [
    ProtocolVersion::V_2025_11_25,
    ProtocolVersion::V_2025_06_18,
    ProtocolVersion::V_2025_03_26,
    ProtocolVersion::V_2024_11_05,
];

/// The server's name in `serverInfo`.
pub const SERVER_NAME: &str = "oac";

/// The capabilities of a channel-path server: tools, the channel capability, and the OAC
/// extension identifier with an empty settings object ([MCPB-EXT-003]). Nothing else.
pub fn capabilities() -> ServerCapabilities {
    let mut caps = ServerCapabilities::default();
    caps.tools = Some(ToolsCapability::default());
    caps.experimental = Some(BTreeMap::from([(
        CHANNEL_CAPABILITY.to_owned(),
        JsonObject::new(),
    )]));
    caps.extensions = Some(BTreeMap::from([(
        EXTENSION_ID_V0.to_owned(),
        JsonObject::new(),
    )]));
    caps
}

/// The MCP server for one attachment.
pub struct ChannelServer {
    pub(crate) inner: Arc<Inner>,
    pub(crate) attachment: Attachment,
}

impl ServerHandler for ChannelServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(capabilities())
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_server_info(Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Owned(LEGACY_REVISIONS.to_vec())
    }

    async fn discover(
        &self,
        _context: RequestContext<RoleServer>,
    ) -> Result<DiscoverResult, ErrorData> {
        Err(ErrorData::method_not_found::<DiscoverRequestMethod>())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(oac_mcp_tools::tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let call = match oac_mcp_tools::parse(&request.name, request.arguments.as_ref()) {
            Ok(c) => c,
            Err(CallError::UnknownTool(_)) => return Err(oac_mcp_tools::unknown_tool()),
            Err(CallError::Invalid(_)) => {
                return Ok(oac_mcp_tools::refusal(ErrorCode::InvalidRequest).into());
            }
        };
        // The core's sink is synchronous; it runs off the connection's runtime thread.
        let inner = self.inner.clone();
        let attachment = self.attachment.clone();
        let result = tokio::task::spawn_blocking(move || inner.request(&attachment, call))
            .await
            .unwrap_or_else(|_| oac_mcp_tools::refusal(ErrorCode::InternalError));
        Ok(result.into())
    }
}

/// `rmcp`'s byte-stream transport over the connection, with the channel path's two
/// additions: the `server/discover` answer, and the attachment opening once the first
/// `tools/list` answer is written.
pub struct ChannelTransport {
    inner: AsyncRwTransport<RoleServer, ChannelReader, ChannelWriter>,
    adapter: Arc<Inner>,
    attachment: Attachment,
}

impl ChannelTransport {
    /// The transport over the two halves of one connection.
    pub fn new(
        read: ChannelReader,
        write: ChannelWriter,
        adapter: Arc<Inner>,
        attachment: Attachment,
    ) -> ChannelTransport {
        ChannelTransport {
            inner: AsyncRwTransport::new_server(read, write),
            adapter,
            attachment,
        }
    }
}

impl Transport<RoleServer> for ChannelTransport {
    type Error = std::io::Error;

    fn send(
        &mut self,
        item: ServerJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let listed = matches!(
            &item,
            JsonRpcMessage::Response(r) if matches!(r.result, ServerResult::ListToolsResult(_))
        );
        let sent = self.inner.send(item);
        let adapter = self.adapter.clone();
        let attachment = self.attachment.clone();
        async move {
            let r = sent.await;
            if listed && r.is_ok() {
                adapter.listed(&attachment);
            }
            r
        }
    }

    async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
        loop {
            let msg = self.inner.receive().await?;
            if let JsonRpcMessage::Request(req) = &msg
                && matches!(req.request, ClientRequest::DiscoverRequest(_))
            {
                // [MCPB-CLD-003]: `-32601`, a JSON-RPC error that is not a recognized modern
                // one, keeps a dual-era client on the legacy era.
                let answer = ServerJsonRpcMessage::error(
                    ErrorData::method_not_found::<DiscoverRequestMethod>(),
                    Some(req.id.clone()),
                );
                if self.inner.send(answer).await.is_err() {
                    return None;
                }
                continue;
            }
            return Some(msg);
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        self.inner.close().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oac_core::json::{self, Json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    use crate::channel::PERMISSION_RELAY_CAPABILITY;

    /// C6 §7, C10, 06 row 11: permission relay is off. `experimental` holds the channel
    /// capability alone, so `claude/channel/permission` is never declared; and no resource,
    /// prompt, logging or completion capability is offered ([MCPB-DLV-002]).
    #[test]
    fn permission_relay_is_not_declared_and_nothing_is_pollable() {
        let caps = capabilities();
        let experimental: Vec<&str> = caps
            .experimental
            .as_ref()
            .expect("experimental")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(experimental, [CHANNEL_CAPABILITY]);
        assert!(!experimental.contains(&PERMISSION_RELAY_CAPABILITY));
        assert!(caps.resources.is_none() && caps.prompts.is_none());
        assert!(caps.logging.is_none() && caps.completions.is_none());
    }

    /// A server that answers the probe with `-32601` from its own handler, as the G4 spike
    /// did, but is served by `rmcp`'s `serve_server` with no transport of its own. `legacy`
    /// narrows its versions to the legacy ones, as [`ChannelServer`] does.
    struct Bare {
        legacy: bool,
    }

    impl ServerHandler for Bare {
        fn get_info(&self) -> ServerConfig {
            ServerConfig::new(capabilities()).with_protocol_version(ProtocolVersion::V_2025_11_25)
        }
        fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
            if self.legacy {
                Cow::Owned(LEGACY_REVISIONS.to_vec())
            } else {
                Cow::Borrowed(ProtocolVersion::KNOWN_VERSIONS)
            }
        }
        async fn discover(
            &self,
            _context: RequestContext<RoleServer>,
        ) -> Result<DiscoverResult, ErrorData> {
            Err(ErrorData::method_not_found::<DiscoverRequestMethod>())
        }
    }

    fn error_code(line: &str) -> Option<String> {
        let v = json::parse(line.as_bytes()).expect("a JSON frame");
        let e = v.as_object()?.get("error")?.as_object()?.get("code")?;
        match e {
            Json::Number(n) => Some(n.raw().to_owned()),
            _ => None,
        }
    }

    /// Opens `server` the way Claude Code `2.1.283` does (D6 lines 3-9): the probe, then on
    /// an error `initialize`, `notifications/initialized` and `tools/list`. Returns the
    /// probe's error code and the listing's error code, if any.
    fn open_like_claude_code(server: Bare) -> (Option<String>, Option<String>) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a runtime");
        rt.block_on(async {
            let (client, served) = tokio::io::duplex(1 << 16);
            let (sr, sw) = tokio::io::split(served);
            let serving = tokio::spawn(async move { rmcp::serve_server(server, (sr, sw)).await });
            let (cr, mut cw) = tokio::io::split(client);
            let mut lines = BufReader::new(cr).lines();
            let probe = "{\"jsonrpc\":\"2.0\",\"id\":\"server-discover-probe-1\",\"method\":\"server/discover\",\"params\":{\"_meta\":{\"io.modelcontextprotocol/protocolVersion\":\"2026-07-28\",\"io.modelcontextprotocol/clientInfo\":{\"name\":\"claude-code\",\"version\":\"2.1.283\"},\"io.modelcontextprotocol/clientCapabilities\":{}}}}\n";
            cw.write_all(probe.as_bytes()).await.expect("write");
            let answer = lines.next_line().await.expect("read").expect("a line");
            let init = "{\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"claude-code\",\"version\":\"2.1.283\"}},\"jsonrpc\":\"2.0\",\"id\":0}\n";
            cw.write_all(init.as_bytes()).await.expect("write");
            let _init_answer = lines.next_line().await.expect("read").expect("a line");
            cw.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
                .await
                .expect("write");
            cw.write_all(b"{\"method\":\"tools/list\",\"jsonrpc\":\"2.0\",\"id\":1}\n")
                .await
                .expect("write");
            let listing = lines.next_line().await.expect("read").expect("a line");
            drop(cw);
            drop(lines);
            let _ = serving.await;
            (error_code(&answer), error_code(&listing))
        })
    }

    /// Why [`ChannelTransport`] answers the probe itself. Served by `rmcp` `3.4.0`'s
    /// `serve_server` alone, a channel-path server cannot follow D6's opening:
    ///
    /// - with only the legacy revisions supported, `rmcp` answers the probe itself with
    ///   `-32022` (`UnsupportedProtocolVersionError`), a recognized modern error that
    ///   [MCPB-CLD-003] forbids, before the handler's `discover` runs;
    /// - with the modern revision supported too, the handler's `-32601` goes out, but the
    ///   connection stays on the stateless era after the fallback to `initialize`, and the
    ///   legacy `tools/list` is refused with `-32602` for want of per-request `_meta`.
    #[test]
    fn rmcp_alone_cannot_serve_the_probe_then_legacy_opening() {
        assert_eq!(
            open_like_claude_code(Bare { legacy: true }).0.as_deref(),
            Some("-32022")
        );
        assert_eq!(
            open_like_claude_code(Bare { legacy: false }),
            (Some("-32601".to_owned()), Some("-32602".to_owned()))
        );
    }
}
