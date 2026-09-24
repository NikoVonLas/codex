use super::App;
use crate::app_server_session::AppServerSession;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::PeerGroup;
use codex_app_server_protocol::ThreadPeerGroupParams;
use codex_app_server_protocol::ThreadPeerGroupResponse;
use codex_protocol::ThreadId;

impl App {
    pub(super) async fn update_peer_group(
        &mut self,
        app_server: &mut AppServerSession,
        thread_id: ThreadId,
        selection: Option<PeerGroup>,
    ) {
        let request_id = app_server.next_request_id();
        let result = app_server
            .request_handle()
            .request_typed::<ThreadPeerGroupResponse>(ClientRequest::ThreadPeerGroup {
                request_id,
                params: ThreadPeerGroupParams {
                    thread_id: thread_id.to_string(),
                    selection,
                },
            })
            .await;
        if self.current_displayed_thread_id() != Some(thread_id) {
            return;
        }
        match result {
            Ok(response) => {
                self.chat_widget.show_peer_group(&response.selection);
            }
            Err(error) => self
                .chat_widget
                .add_error_message(format!("Could not update group: {error}")),
        }
    }
}
