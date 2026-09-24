//! Cross-process mailboxes for independent local roots. Membership is host-selected.

mod control;
mod extension;
pub use control::PeerGroup;
pub use control::peer_group;
mod tools;

use codex_protocol::ThreadId;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_state::SqliteConfig;
pub use extension::PeerOptions;
pub use extension::install_peers;
use serde::Serialize;
use sqlx::Row;
use sqlx::SqlitePool;
use std::sync::atomic::AtomicBool;
use uuid::Uuid;

const MAX_MESSAGE_BYTES: usize = 512;
/// A bounded message attributed to an independent session, never to the user.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PeerMessage {
    pub id: i64,
    pub sender: String,
    pub text: String,
}

/// One runtime's lease on a durable mailbox. Separate processes open independent pools.
pub struct PeerMailbox {
    pool: SqlitePool,
    id: String,
    lease: String,
    available: AtomicBool,
    has_repository: bool,
}

impl PeerMailbox {
    pub async fn open(
        sqlite: &SqliteConfig,
        id: ThreadId,
        repository: Option<String>,
        group: Option<String>,
    ) -> Result<Self> {
        if group
            .as_ref()
            .is_some_and(|group| group.trim().is_empty() || group.len() > 128)
        {
            return Err(invalid(
                "peer_group must contain 1 to 128 bytes of nonblank text",
            ));
        }
        tokio::fs::create_dir_all(sqlite.home()).await?;
        let pool = sqlite
            .open_read_write_pool(&sqlite.home().join("agent_peers_1.sqlite"))
            .await
            .map_err(storage)?;
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS peers (
            id TEXT PRIMARY KEY, lease TEXT NOT NULL, repository TEXT, group_name TEXT, seen INTEGER NOT NULL, enabled INTEGER NOT NULL DEFAULT 1, user_selected INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT, sender TEXT NOT NULL, recipient TEXT NOT NULL,
            text TEXT NOT NULL, created INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS peer_inbox ON messages(recipient,id);")
            .execute(&pool).await.map_err(storage)?;
        let mailbox = Self {
            pool,
            id: id.to_string(),
            lease: Uuid::now_v7().to_string(),
            available: AtomicBool::new(/*v*/ false),
            has_repository: repository.is_some(),
        };
        // Rebinding a resumed session must not expose messages from its previous project/group.
        let mut tx = mailbox
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(storage)?;
        sqlx::query(
            "DELETE FROM messages WHERE recipient=? AND EXISTS (
            SELECT 1 FROM peers WHERE id=? AND (repository IS NOT ? OR (user_selected=0 AND group_name IS NOT ?)))",
        )
        .bind(&mailbox.id)
        .bind(&mailbox.id)
        .bind(&repository)
        .bind(&group)
        .execute(&mut *tx)
        .await
        .map_err(storage)?;
        sqlx::query("INSERT INTO peers(id,lease,repository,group_name,seen) VALUES (?,?,?,?,unixepoch()) ON CONFLICT(id) DO UPDATE SET
            lease=excluded.lease, repository=excluded.repository,
            group_name=CASE WHEN peers.user_selected=0 THEN excluded.group_name ELSE peers.group_name END, seen=excluded.seen")
            .bind(&mailbox.id).bind(&mailbox.lease).bind(repository).bind(group)
            .execute(&mut *tx).await.map_err(storage)?;
        tx.commit().await.map_err(storage)?;
        let available: bool = sqlx::query_scalar("SELECT enabled=1 AND (repository IS NOT NULL OR group_name IS NOT NULL) FROM peers WHERE id=?")
            .bind(&mailbox.id).fetch_one(&mailbox.pool).await.map_err(storage)?;
        mailbox
            .available
            .store(available, std::sync::atomic::Ordering::Relaxed);
        Ok(mailbox)
    }

    pub async fn heartbeat(&self) -> Result<bool> {
        let updated = sqlx::query("UPDATE peers SET seen=unixepoch() WHERE id=? AND lease=?")
            .bind(&self.id)
            .bind(&self.lease)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        // Bound retention independently of delivery, including after recipient crashes.
        sqlx::query("DELETE FROM messages WHERE created < unixepoch()-86400")
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(updated.rows_affected() == 1)
    }

    pub async fn list(&self, after: &str) -> Result<Vec<String>> {
        let query = "SELECT b.id FROM peers a JOIN peers b ON a.enabled=1 AND b.enabled=1 AND a.id != b.id AND b.seen >= unixepoch()-30 AND
 ((a.repository IS NOT NULL AND a.repository=b.repository) OR
 (a.group_name IS NOT NULL AND a.group_name=b.group_name))
            WHERE a.id=? AND a.lease=? AND b.id>? ORDER BY b.id LIMIT 20";
        sqlx::query_scalar(query)
            .bind(&self.id)
            .bind(&self.lease)
            .bind(after)
            .fetch_all(&self.pool)
            .await
            .map_err(storage)
    }

    pub async fn send(&self, recipient: &str, text: &str) -> Result<()> {
        if text.trim().is_empty() || text.len() > MAX_MESSAGE_BYTES {
            return Err(invalid(
                "peer messages must contain 1 to 512 bytes of nonblank text",
            ));
        }
        let query = "INSERT INTO messages(sender,recipient,text,created)
            SELECT a.id,b.id,?,unixepoch() FROM peers a JOIN peers b ON a.enabled=1 AND b.enabled=1 AND a.id != b.id AND b.seen >= unixepoch()-30 AND
 ((a.repository IS NOT NULL AND a.repository=b.repository) OR
 (a.group_name IS NOT NULL AND a.group_name=b.group_name))
            WHERE a.id=? AND a.lease=? AND b.id=?
            AND (SELECT count(*) FROM messages WHERE recipient=b.id) < 64";
        let result = sqlx::query(query)
            .bind(text)
            .bind(&self.id)
            .bind(&self.lease)
            .bind(recipient)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        if result.rows_affected() != 1 {
            return Err(invalid(
                "peer is unavailable, outside your repository/group, or its inbox is full",
            ));
        }
        Ok(())
    }

    pub async fn pending(&self) -> Result<Option<PeerMessage>> {
        let row = sqlx::query(
            "SELECT m.id,m.sender,m.text FROM messages m JOIN peers p ON p.id=m.recipient
            WHERE p.id=? AND p.lease=? AND p.enabled=1 ORDER BY m.id LIMIT 1",
        )
        .bind(&self.id)
        .bind(&self.lease)
        .fetch_optional(&self.pool)
        .await
        .map_err(storage)?;
        Ok(row.map(|row| PeerMessage {
            id: row.get(/*index*/ 0),
            sender: row.get(/*index*/ 1),
            text: row.get(/*index*/ 2),
        }))
    }

    pub async fn acknowledge(&self, id: i64) -> Result<()> {
        sqlx::query(
            "DELETE FROM messages WHERE id=? AND recipient=? AND EXISTS
            (SELECT 1 FROM peers WHERE id=? AND lease=?)",
        )
        .bind(id)
        .bind(&self.id)
        .bind(&self.id)
        .bind(&self.lease)
        .execute(&self.pool)
        .await
        .map_err(storage)?;
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        // Keep the project binding so a later resume can invalidate old queued messages.
        sqlx::query("UPDATE peers SET seen=0 WHERE id=? AND lease=?")
            .bind(&self.id)
            .bind(&self.lease)
            .execute(&self.pool)
            .await
            .map_err(storage)?;
        Ok(())
    }
}

fn invalid(message: &str) -> CodexErr {
    CodexErr::InvalidRequest(message.into())
}
fn storage(error: impl std::fmt::Display) -> CodexErr {
    CodexErr::InvalidRequest(format!("peer messaging storage: {error}"))
}
