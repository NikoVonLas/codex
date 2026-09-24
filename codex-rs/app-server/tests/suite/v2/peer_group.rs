use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::PeerGroup;
use codex_app_server_protocol::ThreadPeerGroupParams;
use codex_app_server_protocol::ThreadPeerGroupResponse;
use codex_app_server_protocol::ThreadStartParams;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

#[test_case::test_case(true; "switch_live_group")]
#[test_case::test_case(false; "respect_project_disable")]
#[tokio::test]
async fn peer_group_commands_apply_to_the_current_thread(enabled: bool) -> anyhow::Result<()> {
    core_test_support::skip_if_remote!(Ok(()), "peer groups connect host-local sessions");
    let server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri())
        .with_extra_config(&format!(
            "[features.multi_agent_v2]\nenabled = true\npeer_messaging = {enabled}"
        ))
        .write(home.path())?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized_with_timeout(std::time::Duration::from_secs(/*secs*/ 30))
        .await?;
    let thread = app.start_thread(ThreadStartParams::default()).await?.thread;
    if !enabled {
        let request = app
            .send_request(
                "thread/peerGroup",
                Some(serde_json::json!({
                    "threadId": thread.id, "selection": {"type":"named", "name":"website"}
                })),
            )
            .await?;
        let error = app
            .read_stream_until_error_message(codex_app_server_protocol::RequestId::Integer(request))
            .await?;
        assert!(error.error.message.contains("disabled"));
        return Ok(());
    }
    for selection in [
        PeerGroup::Named {
            name: "website".into(),
        },
        PeerGroup::Off,
        PeerGroup::Auto,
    ] {
        let result = app
            .request::<ThreadPeerGroupResponse>(|request_id| ClientRequest::ThreadPeerGroup {
                request_id,
                params: ThreadPeerGroupParams {
                    thread_id: thread.id.clone(),
                    selection: Some(selection.clone()),
                },
            })
            .await;
        assert_eq!(
            result?,
            ThreadPeerGroupResponse {
                selection: selection.clone()
            }
        );
        let current: ThreadPeerGroupResponse = app
            .request(|request_id| ClientRequest::ThreadPeerGroup {
                request_id,
                params: ThreadPeerGroupParams {
                    thread_id: thread.id.clone(),
                    selection: None,
                },
            })
            .await?;
        assert_eq!(current, ThreadPeerGroupResponse { selection });
    }
    Ok(())
}
