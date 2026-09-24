use codex_agent_message_board_extension::PeerGroup;
use codex_agent_message_board_extension::PeerMailbox;
use codex_agent_message_board_extension::PeerMessage;
use codex_protocol::ThreadId;
use codex_state::SqliteConfig;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn independent_mailboxes_discover_coordinate_and_isolate() {
    let directory = tempfile::tempdir().unwrap();
    let sqlite = SqliteConfig::new_for_testing(directory.path().to_path_buf().try_into().unwrap());
    let a_id = ThreadId::new();
    let b_id = ThreadId::new();
    let a = PeerMailbox::open(&sqlite, a_id, Some("repo-a".into()), Some("project".into()))
        .await
        .unwrap();
    let b = PeerMailbox::open(&sqlite, b_id, Some("repo-a".into()), /*group*/ None)
        .await
        .unwrap();
    let c_id = ThreadId::new();
    let c = PeerMailbox::open(&sqlite, c_id, Some("repo-b".into()), Some("project".into()))
        .await
        .unwrap();
    let outsider_id = ThreadId::new();
    let _outsider = PeerMailbox::open(
        &sqlite,
        outsider_id,
        Some("repo-c".into()),
        /*group*/ None,
    )
    .await
    .unwrap();
    let mut expected = vec![b_id.to_string(), c_id.to_string()];
    expected.sort();
    assert_eq!(a.list("").await.unwrap(), expected);
    assert_eq!(b.list("").await.unwrap(), vec![a_id.to_string()]);
    assert_eq!(c.list("").await.unwrap(), vec![a_id.to_string()]);
    assert!(a.send(&outsider_id.to_string(), "private").await.is_err());
    assert!(a.send(&a_id.to_string(), "self").await.is_err());
    a.send(&b_id.to_string(), "I am editing the parser")
        .await
        .unwrap();
    let message = b.pending().await.unwrap().unwrap();
    assert_eq!(
        message,
        PeerMessage {
            id: message.id,
            sender: a_id.to_string(),
            text: "I am editing the parser".into()
        }
    );
    // Reading does not consume messages: idle recipients retain them until accepted.
    assert_eq!(b.pending().await.unwrap(), Some(message.clone()));
    b.acknowledge(message.id).await.unwrap();
    assert_eq!(b.pending().await.unwrap(), None);
    assert!(a.send(&b_id.to_string(), "  ").await.is_err());
    assert!(a.send(&b_id.to_string(), &"я".repeat(257)).await.is_err());
    for _ in 0..64 {
        a.send(&b_id.to_string(), "hello").await.unwrap();
    }
    assert!(a.send(&b_id.to_string(), "overflow").await.is_err());
    b.close().await.unwrap();
    assert_eq!(a.list("").await.unwrap(), vec![c_id.to_string()]);
    assert!(a.send(&b_id.to_string(), "offline").await.is_err());
    a.set_group(PeerGroup::Off).await.unwrap();
    assert!(c.send(&a_id.to_string(), "disabled").await.is_err());
    assert_eq!(a.list("").await.unwrap(), Vec::<String>::new());
    a.set_group(PeerGroup::Auto).await.unwrap();
    assert_eq!(a.list("").await.unwrap(), Vec::<String>::new());
    a.set_group(PeerGroup::Named("project".into()))
        .await
        .unwrap();
    assert_eq!(a.list("").await.unwrap(), vec![c_id.to_string()]);
    a.set_group(PeerGroup::Off).await.unwrap();
    let resumed = PeerMailbox::open(&sqlite, a_id, Some("repo-a".into()), Some("project".into()))
        .await
        .unwrap();
    assert_eq!(resumed.group().await.unwrap(), PeerGroup::Off);
}

#[tokio::test]
async fn resumed_scope_and_runtime_lease_protect_delivery() {
    let directory = tempfile::tempdir().unwrap();
    let sqlite = SqliteConfig::new_for_testing(directory.path().to_path_buf().try_into().unwrap());
    let a_id = ThreadId::new();
    let b_id = ThreadId::new();
    let a = PeerMailbox::open(
        &sqlite,
        a_id,
        /*repository*/ None,
        Some("project".into()),
    )
    .await
    .unwrap();
    let b = PeerMailbox::open(
        &sqlite,
        b_id,
        /*repository*/ None,
        Some("project".into()),
    )
    .await
    .unwrap();
    a.send(&b_id.to_string(), "queued").await.unwrap();
    let resumed = PeerMailbox::open(
        &sqlite,
        b_id,
        /*repository*/ None,
        Some("project".into()),
    )
    .await
    .unwrap();
    assert!(!b.heartbeat().await.unwrap());
    b.close().await.unwrap();
    assert_eq!(a.list("").await.unwrap(), vec![b_id.to_string()]);
    assert_eq!(b.pending().await.unwrap(), None);
    assert!(resumed.pending().await.unwrap().is_some());
    let moved = PeerMailbox::open(
        &sqlite,
        b_id,
        /*repository*/ None,
        Some("different".into()),
    )
    .await
    .unwrap();
    assert_eq!(moved.pending().await.unwrap(), None);
    assert_eq!(a.list("").await.unwrap(), Vec::<String>::new());
}
