//! Host adapter for repository-scoped communication between independent local roots.
use crate::ThreadManager;
use crate::config::Config;
use crate::context::ContextualUserFragment;
use crate::context::PeerMessageFragment;
use codex_agent_message_board_extension::PeerOptions;
use codex_exec_server::LOCAL_FS;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_features::Feature;
use std::sync::Weak;

pub(crate) fn install(
    registry: &mut ExtensionRegistryBuilder<Config>,
    manager: Weak<ThreadManager>,
) {
    codex_agent_message_board_extension::install_peers(
        registry,
        |config: &Config, environments| {
            let config = config.clone();
            let cwd = environments
                .first()
                .map(|environment| environment.cwd.to_abs_path());
            Box::pin(async move {
                if config.ephemeral
                    || !config.features.enabled(Feature::MultiAgentV2)
                    || !config.multi_agent_v2.peer_messaging
                    || config.multi_agent_v2.disable_direct_message
                {
                    return None;
                }
                let cwd = match cwd {
                    Some(Ok(cwd)) => cwd,
                    Some(Err(_)) => return None,
                    None => config.cwd.clone(),
                };
                let repository = match codex_git_utils::resolve_root_git_project_for_trust(
                    LOCAL_FS.as_ref(),
                    &cwd,
                )
                .await
                {
                    Some(root) => tokio::fs::canonicalize(root.as_path())
                        .await
                        .ok()
                        .and_then(|path| path.to_str().map(str::to_owned)),
                    None => None,
                };
                Some(PeerOptions {
                    sqlite: config.sqlite_config().clone(),
                    repository,
                    group: config.multi_agent_v2.peer_group,
                })
            })
        },
        move |recipient, message| {
            let manager = manager.clone();
            Box::pin(async move {
                let Some(manager) = manager.upgrade() else {
                    return Ok(false);
                };
                let Ok(thread) = manager.get_thread(recipient).await else {
                    return Ok(false);
                };
                if !thread.config().await.multi_agent_v2.peer_messaging {
                    return Ok(false);
                }
                Ok(thread
                    .inject_if_running(vec![ContextualUserFragment::into(PeerMessageFragment(
                        message,
                    ))])
                    .await
                    .is_ok())
            })
        },
    );
}
