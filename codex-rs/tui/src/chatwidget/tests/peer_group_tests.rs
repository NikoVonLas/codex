use super::helpers::drain_insert_history;
use super::helpers::make_chatwidget_manual;
use super::*;
use codex_app_server_protocol::PeerGroup;

#[tokio::test]
async fn group_commands_target_the_current_session_without_starting_a_turn() {
    let (mut chat, mut events, mut turns) = make_chatwidget_manual(/*model_override*/ None).await;
    let thread_id = ThreadId::new();
    chat.thread_id = Some(thread_id);
    for (input, expected) in [
        ("", None),
        ("auto", Some(PeerGroup::Auto)),
        ("off", Some(PeerGroup::Off)),
        (
            "website",
            Some(PeerGroup::Named {
                name: "website".into(),
            }),
        ),
    ] {
        chat.request_peer_group(input);
        let event = std::iter::from_fn(|| events.try_recv().ok()).find_map(|event| match event {
            AppEvent::PeerGroup {
                thread_id,
                selection,
            } => Some((thread_id, selection)),
            _ => None,
        });
        assert_eq!(event, Some((thread_id, expected)));
    }
    assert!(turns.try_recv().is_err());
}

#[tokio::test]
async fn group_status_is_readable() {
    let (mut chat, mut events, _) = make_chatwidget_manual(/*model_override*/ None).await;
    for mode in [
        PeerGroup::Auto,
        PeerGroup::Named {
            name: "website".into(),
        },
        PeerGroup::Off,
    ] {
        chat.show_peer_group(&mode);
    }
    assert_chatwidget_snapshot!(
        "peer_group_status",
        drain_insert_history(&mut events)
            .into_iter()
            .flatten()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
