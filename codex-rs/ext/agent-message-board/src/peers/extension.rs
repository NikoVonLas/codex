use super::PeerMailbox;
use super::PeerMessage;
use super::tools::PeerTool;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionEventSink;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::ExtensionWarning;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::ThreadReadyInput;
use codex_extension_api::ThreadStartInput;
use codex_extension_api::ThreadStopInput;
use codex_extension_api::ToolContributor;
use codex_protocol::ThreadId;
use codex_protocol::error::Result;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_state::SqliteConfig;
use codex_tools::ToolCall;
use codex_tools::ToolExecutor;
use futures::future::BoxFuture;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Host-selected scope. Neither repository identity nor group is model-controlled.
pub struct PeerOptions {
    pub sqlite: SqliteConfig,
    pub repository: Option<String>,
    pub group: Option<String>,
}
type Options<C> = dyn Fn(&C, &[TurnEnvironmentSelection]) -> BoxFuture<'static, Option<PeerOptions>>
    + Send
    + Sync;
type Deliver = dyn Fn(ThreadId, PeerMessage) -> BoxFuture<'static, Result<bool>> + Send + Sync;
struct Peers<C> {
    options: Box<Options<C>>,
    deliver: Arc<Deliver>,
    events: Arc<dyn ExtensionEventSink>,
}
pub(super) struct Binding {
    pub(super) mailbox: Arc<PeerMailbox>,
    pub(super) gate: Arc<Mutex<()>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl Drop for Binding {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.get_mut().take() {
            worker.abort();
        }
    }
}
impl<C: Sync> ThreadLifecycleContributor<C> for Peers<C> {
    fn on_thread_start<'a>(&'a self, input: ThreadStartInput<'a, C>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            // Independent roots participate; their children retain existing tree-local messaging.
            // The mailbox owns its storage; history database availability must not gate peers.
            // The host options callback excludes ephemeral sessions.
            if input.session_source.parent_thread_id().is_some()
                || input
                    .environments
                    .iter()
                    .any(|environment| environment.environment_id != "local")
            {
                return;
            }
            let Some(options) = (self.options)(input.config, input.environments).await else {
                return;
            };
            let Ok(id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            match PeerMailbox::open(&options.sqlite, id, options.repository, options.group).await {
                Ok(mailbox) => {
                    input.thread_store.insert(Binding {
                        mailbox: Arc::new(mailbox),
                        gate: Arc::new(Mutex::new(())),
                        worker: Mutex::new(/*t*/ None),
                    });
                }
                Err(_) => self.events.emit_warning(ExtensionWarning {
                    thread_id: id.to_string(),
                    turn_id: None,
                    message:
                        "Local peer messaging could not initialize; peer tools are unavailable."
                            .into(),
                }),
            }
        })
    }
    #[expect(
        clippy::await_holding_invalid_type,
        reason = "serialize delivery with explicit group changes"
    )]
    fn on_thread_ready<'a>(&'a self, input: ThreadReadyInput<'a, C>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            let Some(binding) = input.thread_store.get::<Binding>() else {
                return;
            };
            let Ok(id) = ThreadId::from_string(input.thread_store.level_id()) else {
                return;
            };
            let mut worker = binding.worker.lock().await;
            if worker.is_some() {
                return;
            }
            let mailbox = binding.mailbox.clone();
            let deliver = self.deliver.clone();
            let gate = binding.gate.clone();
            *worker = Some(tokio::spawn(async move {
                loop {
                    let guard = gate.lock().await;
                    match mailbox.heartbeat().await {
                        Ok(true) => {}
                        Ok(false) => break,
                        Err(error) => {
                            tracing::warn!(%error, "Peer heartbeat failed");
                        }
                    }
                    if let Ok(Some(message)) = mailbox.pending().await {
                        let message_id = message.id;
                        if matches!(deliver(id, message).await, Ok(true))
                            && let Err(error) = mailbox.acknowledge(message_id).await
                        {
                            tracing::warn!(%error, "Peer acknowledgement failed");
                        }
                    }
                    drop(guard);
                    tokio::time::sleep(Duration::from_secs(/*secs*/ 1)).await;
                }
            }));
        })
    }
    fn on_thread_stop<'a>(&'a self, input: ThreadStopInput<'a>) -> ExtensionFuture<'a, ()> {
        Box::pin(async move {
            if let Some(binding) = input.thread_store.get::<Binding>() {
                let worker = binding.worker.lock().await.take();
                if let Some(worker) = worker {
                    worker.abort();
                    let _ = worker.await;
                }
                if let Err(error) = binding.mailbox.close().await {
                    tracing::warn!(%error, "Peer close failed");
                }
            }
        })
    }
}
impl<C: Sync> ToolContributor for Peers<C> {
    fn tools(
        &self,
        _session: &ExtensionData,
        thread: &ExtensionData,
    ) -> Vec<Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>> {
        thread
            .get::<Binding>()
            .filter(|binding| {
                binding
                    .mailbox
                    .available
                    .load(std::sync::atomic::Ordering::Relaxed)
            })
            .map_or_else(Vec::new, |binding| {
                [false, true]
                    .into_iter()
                    .map(|send| {
                        Arc::new(PeerTool {
                            mailbox: binding.mailbox.clone(),
                            send,
                        })
                            as Arc<dyn for<'call> ToolExecutor<ToolCall<'call>>>
                    })
                    .collect()
            })
    }
}

/// Registers local cross-process messaging. Delivery accepts context only while the root is active.
pub fn install_peers<C: Sync + 'static>(
    registry: &mut ExtensionRegistryBuilder<C>,
    options: impl Fn(&C, &[TurnEnvironmentSelection]) -> BoxFuture<'static, Option<PeerOptions>>
    + Send
    + Sync
    + 'static,
    deliver: impl Fn(ThreadId, PeerMessage) -> BoxFuture<'static, Result<bool>> + Send + Sync + 'static,
) {
    let peers = Arc::new(Peers {
        options: Box::new(options),
        deliver: Arc::new(deliver),
        events: registry.event_sink(),
    });
    registry.thread_lifecycle_contributor(peers.clone());
    registry.tool_contributor(peers);
}
