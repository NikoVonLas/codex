use super::PeerMailbox;
use super::extension::Binding;
use super::invalid;
use super::storage;
use codex_extension_api::ExtensionData;
use codex_protocol::error::Result;

/// A user-selected scope for independent sessions. Named groups also retain repository peers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PeerGroup {
    Auto,
    Off,
    Named(String),
}

/// Reads or updates a live root's group, ordered against message delivery.
#[expect(
    clippy::await_holding_invalid_type,
    reason = "group changes must be atomic with delivery"
)]
pub async fn peer_group(data: &ExtensionData, selection: Option<PeerGroup>) -> Result<PeerGroup> {
    let binding = data.get::<Binding>().ok_or_else(|| {
        invalid("Peer messaging is disabled by configuration or unavailable for this session.")
    })?;
    let _guard = binding.gate.lock().await;
    if let Some(selection) = selection {
        binding.mailbox.set_group(selection).await?;
    }
    binding.mailbox.group().await
}

impl PeerMailbox {
    pub async fn group(&self) -> Result<PeerGroup> {
        let (enabled, name): (bool, Option<String>) =
            sqlx::query_as("SELECT enabled,group_name FROM peers WHERE id=? AND lease=?")
                .bind(&self.id)
                .bind(&self.lease)
                .fetch_one(&self.pool)
                .await
                .map_err(storage)?;
        Ok(match (enabled, name) {
            (false, _) => PeerGroup::Off,
            (true, Some(name)) => PeerGroup::Named(name),
            (true, None) => PeerGroup::Auto,
        })
    }
    pub async fn set_group(&self, selection: PeerGroup) -> Result<()> {
        let (enabled, name) = match selection {
            PeerGroup::Auto => (true, None),
            PeerGroup::Off => (false, None),
            PeerGroup::Named(name) => {
                if name.trim().is_empty() || name.len() > 128 {
                    return Err(invalid(
                        "Group names must contain 1 to 128 bytes of nonblank text",
                    ));
                }
                (true, Some(name))
            }
        };
        let available = enabled && (name.is_some() || self.has_repository);
        let mut tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(storage)?;
        sqlx::query(
            "DELETE FROM messages WHERE recipient=? AND EXISTS
            (SELECT 1 FROM peers WHERE id=? AND lease=? AND (enabled != ? OR group_name IS NOT ?))",
        )
        .bind(&self.id)
        .bind(&self.id)
        .bind(&self.lease)
        .bind(enabled)
        .bind(&name)
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        let changed = sqlx::query(
            "UPDATE peers SET enabled=?,group_name=?,user_selected=1 WHERE id=? AND lease=?",
        )
        .bind(enabled)
        .bind(name)
        .bind(&self.id)
        .bind(&self.lease)
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        if changed.rows_affected() != 1 {
            return Err(invalid("This session no longer owns its peer mailbox"));
        }
        tx.commit().await.map_err(storage)?;
        self.available
            .store(available, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
}
