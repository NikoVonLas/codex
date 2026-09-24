//! Independent roots share a host-local mailbox only when configuration permits it.
use codex_agent_message_board_extension::PeerMailbox;
use codex_features::Feature;
use codex_protocol::ThreadId;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test_case::test_case(true, true; "explicit_group")]
#[test_case::test_case(true, false; "automatic_repository")]
#[test_case::test_case(false, true; "explicitly_disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_peer_tools_respect_project_opt_out(
    enabled: bool,
    use_group: bool,
) -> anyhow::Result<()> {
    core_test_support::skip_if_remote!(
        Ok(()),
        "peer messaging connects local execution environments"
    );
    let server = responses::start_mock_server().await;
    let test = test_codex()
        .with_config(move |config| {
            config
                .features
                .enable(Feature::MultiAgentV2)
                .expect("enable multi-agent");
            config.multi_agent_v2.peer_messaging = enabled;
            config.multi_agent_v2.peer_group = use_group.then(|| "integration-project".into());
        })
        .with_workspace_setup(|cwd, _fs| async move {
            let status = tokio::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(cwd)
                .status()
                .await?;
            anyhow::ensure!(status.success(), "initialize repository");
            Ok(())
        })
        .build_with_auto_env(&server)
        .await?;
    let peer_id = ThreadId::new();
    let peer = PeerMailbox::open(
        test.config.sqlite_config(),
        peer_id,
        Some(
            std::fs::canonicalize(test.workspace_path(""))?
                .to_string_lossy()
                .into_owned(),
        ),
        use_group.then(|| "integration-project".into()),
    )
    .await?;
    if enabled {
        peer.send(
            &test.session_configured.session_id.to_string(),
            "I will update the tests.",
        )
        .await?;
    }
    let done = responses::sse(vec![responses::ev_completed("done")]);
    let bodies = if enabled {
        vec![
            responses::sse(vec![
                responses::ev_function_call(
                    "send",
                    "send_peer_message",
                    &json!({"target":peer_id.to_string(),"message":"I will update the parser."})
                        .to_string(),
                ),
                responses::ev_completed("send"),
            ]),
            done,
        ]
    } else {
        vec![done]
    };
    let mock = responses::mount_response_sequence(
        &server,
        bodies
            .into_iter()
            .enumerate()
            .map(|(index, body)| {
                let response =
                    wiremock::ResponseTemplate::new(200).set_body_raw(body, "text/event-stream");
                if enabled && index == 0 {
                    // Keep inference active across a mailbox poll; receiving must not start an idle turn.
                    response.set_delay(std::time::Duration::from_secs(/*secs*/ 2))
                } else {
                    response
                }
            })
            .collect(),
    )
    .await;
    test.submit_turn("Coordinate the parser change with the other session.")
        .await?;
    let requests = mock.requests();
    if enabled {
        assert!(requests[1].message_input_texts("user").iter().any(|text| {
            text.contains("<peer_message>") && text.contains("I will update the tests.")
        }));
    }
    let tools = requests[0].body_json()["tools"].clone();
    assert_eq!(
        tools
            .as_array()
            .expect("tool list")
            .iter()
            .any(|tool| tool["name"] == "send_peer_message"),
        enabled
    );
    assert_eq!(
        peer.list("")
            .await?
            .contains(&test.session_configured.session_id.to_string()),
        enabled
    );
    assert_eq!(
        peer.pending()
            .await?
            .map(|message| (message.sender, message.text)),
        enabled.then(|| (
            test.session_configured.session_id.to_string(),
            "I will update the parser.".into()
        ))
    );
    let changed = codex_core::peer_group(
        test.codex.thread_extension_data(),
        Some(codex_core::PeerGroup::Off),
    )
    .await;
    assert_eq!(changed.is_ok(), enabled);
    assert!(
        !peer
            .list("")
            .await?
            .contains(&test.session_configured.session_id.to_string())
    );
    Ok(())
}
