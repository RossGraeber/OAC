// SPDX-License-Identifier: Apache-2.0

//! The OAC MCP server for one Codex connection, on `rmcp` (G-7 §3.1; `spec/bindings/mcp.md`
//! §8.2; G8, #69).
//!
//! Codex starts it from a stdio registration (`codex mcp add`) and calls the four tools of
//! `oac-mcp-tools` on it. It is the Codex counterpart of the Claude adapter's channel server
//! and differs from it in what it leaves out:
//!
//! - **The same tools.** `tools/list` is [`oac_mcp_tools::tools`], the one definition both
//!   harnesses read ([MCPB-TOOL-003]).
//! - **No channel capability, and no notification, of any kind** ([MCPB-CDX-001]). Codex has
//!   no MCP surface that turns a notification into session input, so one could only mislead.
//!   Inbound delivery to a Codex session is not MCP (`thread/queue/add`, G7). The server
//!   declares tools and the OAC extension identifier, nothing else ([MCPB-ERA-008]); no
//!   resources, prompts, subscriptions or logging, so nothing a session could read repeatedly
//!   for messages ([MCPB-DLV-002]).
//! - **Legacy era only.** It speaks the legacy revisions ([MCPB-ERA-002]) and answers the
//!   `server/discover` probe with `-32601` as the Claude server does ([MCPB-CLD-003]), so a
//!   client that probes first falls back to `initialize`. Codex's modern era was seen only on
//!   HTTP registrations (G4), which this server does not serve.
//! - **Every call goes through the pairing.** [`CodexMcpServer::call_tool`] hands the parsed
//!   call to [`CodexPairing::serve`], which decides whether to ask the core, after what
//!   confirmation, and renders nothing itself. The `tools/call` `_meta` is not read at all:
//!   no id in it attributes a call to a thread ([MCPB-ATT-008], [SC-ID-162]; see
//!   [`crate::confirm`] on `itemId`).
//!
//! One connection is one thread with a current-thread `tokio` runtime, as in the Claude
//! adapter. Everything inbound is pushed: the harness's frames arrive on the stream. Nothing
//! here asks the harness for anything.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::hash::Hash;
use std::io::{Read, Write};

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

use crate::mcp_io::{StreamReader, StreamWriter};
use crate::pairing::CodexPairing;

/// The legacy revisions this server speaks, newest first ([MCPB-ERA-002]).
pub const LEGACY_REVISIONS: [ProtocolVersion; 4] = [
    ProtocolVersion::V_2025_11_25,
    ProtocolVersion::V_2025_06_18,
    ProtocolVersion::V_2025_03_26,
    ProtocolVersion::V_2024_11_05,
];

/// The server's name in `serverInfo`.
pub const SERVER_NAME: &str = "oac";

/// The capabilities of a Codex-path server: tools and the OAC extension identifier with an
/// empty settings object ([MCPB-EXT-003]). Nothing else: no `experimental` member at all, so
/// no channel capability ([MCPB-CDX-001]).
pub fn capabilities() -> ServerCapabilities {
    let mut caps = ServerCapabilities::default();
    caps.tools = Some(ToolsCapability::default());
    caps.extensions = Some(BTreeMap::from([(
        EXTENSION_ID_V0.to_owned(),
        JsonObject::new(),
    )]));
    caps
}

/// The MCP server of one connection `key`.
pub struct CodexMcpServer<K> {
    pairing: CodexPairing<K>,
    key: K,
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> ServerHandler for CodexMcpServer<K> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(capabilities())
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_server_info(Implementation::new(SERVER_NAME, env!("CARGO_PKG_VERSION")))
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
        // The arguments as the core's JSON, for the comparison with what Codex reports. The
        // `_meta` of the request is not read.
        let arguments = oac_mcp_tools::arguments_json(request.arguments.as_ref());
        let tool = request.name.to_string();
        // The pairing blocks (a thread listing, the wait for a confirmation, the core's
        // synchronous sink): it runs off the connection's runtime thread.
        let pairing = self.pairing.clone();
        let key = self.key.clone();
        let result =
            tokio::task::spawn_blocking(move || pairing.serve(&key, &tool, arguments, call))
                .await
                .unwrap_or_else(|_| oac_mcp_tools::refusal(ErrorCode::InternalError));
        Ok(result.into())
    }
}

/// `rmcp`'s byte-stream transport, with two additions: the `server/discover` answer
/// ([MCPB-CLD-003]), and a report to the port when the first `tools/list` answer is written.
struct CodexTransport<K> {
    inner: AsyncRwTransport<RoleServer, StreamReader, StreamWriter>,
    pairing: CodexPairing<K>,
    key: K,
    listed: bool,
}

impl<K: Clone + Eq + Hash + Send + Sync + 'static> Transport<RoleServer> for CodexTransport<K> {
    type Error = std::io::Error;

    fn send(
        &mut self,
        item: ServerJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let first_listing = !self.listed
            && matches!(
                &item,
                JsonRpcMessage::Response(r) if matches!(r.result, ServerResult::ListToolsResult(_))
            );
        if first_listing {
            self.listed = true;
        }
        let sent = self.inner.send(item);
        let pairing = self.pairing.clone();
        let key = self.key.clone();
        async move {
            let r = sent.await;
            if first_listing && r.is_ok() {
                pairing.port_listed(&key);
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

/// A running connection: stop it with [`ServerHandle::stop`], wait for it with
/// [`ServerHandle::join`].
pub struct ServerHandle {
    stop: tokio::sync::watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ServerHandle {
    /// End the connection: its server stops and its stream is dropped. Returns at once.
    pub fn stop(&self) {
        let _ = self.stop.send(true);
    }

    /// Wait for the connection's thread to end (after the stream ended or [`Self::stop`]).
    pub fn join(mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Serve connection `key`, whose stream is `reader` and `writer`, on a thread of its own,
/// until the stream ends or the handle stops it. When it ends the pairing forgets the
/// connection and the port is told ([`crate::pairing::CallPort::ended`]).
///
/// # Errors
///
/// The error of starting the thread or the reading thread.
pub fn serve_connection<K: Clone + Eq + Hash + Send + Sync + 'static>(
    key: K,
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    pairing: CodexPairing<K>,
) -> std::io::Result<ServerHandle> {
    let (stop, mut stop_rx) = tokio::sync::watch::channel(false);
    let read = StreamReader::spawn(reader)?;
    let thread = std::thread::Builder::new()
        .name("oac-codex-mcp".into())
        .spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
            {
                runtime.block_on(async {
                    let transport = CodexTransport {
                        inner: AsyncRwTransport::new_server(read, StreamWriter(writer)),
                        pairing: pairing.clone(),
                        key: key.clone(),
                        listed: false,
                    };
                    let server = CodexMcpServer {
                        pairing: pairing.clone(),
                        key: key.clone(),
                    };
                    // A connection can be stopped even while its MCP opening waits for input.
                    let opening = tokio::select! {
                        result = rmcp::serve_server(server, transport) => Some(result),
                        _ = stop_rx.changed() => None,
                    };
                    if let Some(Ok(running)) = opening {
                        let cancel = running.cancellation_token();
                        let waiting = running.waiting();
                        tokio::pin!(waiting);
                        tokio::select! {
                            _ = &mut waiting => {},
                            _ = stop_rx.changed() => {
                                cancel.cancel();
                                let _ = waiting.await;
                            }
                        }
                    }
                });
            }
            pairing.connection_closed_and_ended(&key);
        })?;
    Ok(ServerHandle {
        stop,
        thread: Some(thread),
    })
}
