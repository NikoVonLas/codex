use super::*;
use codex_app_server_protocol::PeerGroup;
use codex_app_server_protocol::ThreadPeerGroupParams;
use codex_app_server_protocol::ThreadPeerGroupResponse;

impl TurnRequestProcessor {
    pub(crate) async fn thread_peer_group(
        &self,
        request_id: &ConnectionRequestId,
        params: ThreadPeerGroupParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let (_, thread) = self.load_thread(&params.thread_id).await?;
        self.ensure_direct_input_allowed(request_id, thread.as_ref())
            .await?;
        let selection = params.selection.map(|selection| match selection {
            PeerGroup::Auto => codex_core::PeerGroup::Auto,
            PeerGroup::Off => codex_core::PeerGroup::Off,
            PeerGroup::Named { name } => codex_core::PeerGroup::Named(name),
        });
        let selection = codex_core::peer_group(thread.thread_extension_data(), selection)
            .await
            .map_err(|error| invalid_request(error.to_string()))?;
        let selection = match selection {
            codex_core::PeerGroup::Auto => PeerGroup::Auto,
            codex_core::PeerGroup::Off => PeerGroup::Off,
            codex_core::PeerGroup::Named(name) => PeerGroup::Named { name },
        };
        Ok(Some(ThreadPeerGroupResponse { selection }.into()))
    }
}
