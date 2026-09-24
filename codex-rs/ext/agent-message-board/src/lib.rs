//! Shared agent discussions with interchangeable local and remote backends.
//!
//! Board identity and caller identity come from the host. Implementations own
//! storage and notification fanout; tools and feature registration are separate.

mod api;
mod extension;
mod host;
mod local;
mod peers;
mod tools;
mod types;

pub use api::AgentMessageBoard;
pub use api::ChannelQuery;
pub use api::CreateChannelRequest;
pub use api::PageRequest;
pub use api::PostDestination;
pub use api::PostQuery;
pub use api::PostRequest;
pub use api::ReadPostRequest;
pub use api::ReadThreadRequest;
pub use api::SortDirection;
pub use api::SubscriptionChange;
pub use api::SubscriptionRequest;
pub use api::SubscriptionTarget;
pub use api::ThreadQuery;
pub use api::ThreadSort;
pub use extension::install;
pub use host::MessageBoardHost;
pub use host::NotificationDelivery;
pub use local::LocalAgentMessageBoard;
pub use peers::PeerGroup;
pub use peers::PeerMailbox;
pub use peers::PeerMessage;
pub use peers::PeerOptions;
pub use peers::install_peers;
pub use peers::peer_group;
pub use tools::message_board_tools;
pub use types::ChannelSummary;
pub use types::Page;
pub use types::PostContent;
pub use types::PostMetadata;
pub use types::PostPreview;
pub use types::SubscriptionState;
pub use types::ThreadPage;
pub use types::ThreadSummary;
