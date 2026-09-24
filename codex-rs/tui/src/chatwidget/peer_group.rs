use super::ChatWidget;
use crate::app_event::AppEvent;
use codex_app_server_protocol::PeerGroup;

impl ChatWidget {
    pub(crate) fn show_peer_group(&mut self, selection: &PeerGroup) {
        let message = match selection {
            PeerGroup::Auto => "Group: auto — communicate with sessions in this repository.".into(),
            PeerGroup::Off => {
                "Group: off — communication with independent sessions is disabled.".into()
            }
            PeerGroup::Named { name } => format!(
                "Group: {name} — communicate with this group and sessions in this repository."
            ),
        };
        self.add_info_message(
            message,
            Some("/group <name> · /group auto · /group off".into()),
        );
    }

    pub(super) fn request_peer_group(&mut self, argument: &str) {
        let Some(thread_id) = self.thread_id else {
            self.add_error_message("Start a session before using /group.".into());
            return;
        };
        let selection = match argument.trim() {
            "" => None,
            "auto" => Some(PeerGroup::Auto),
            "off" => Some(PeerGroup::Off),
            name if name.len() <= 128 => Some(PeerGroup::Named { name: name.into() }),
            _ => {
                self.add_error_message("Group names must be at most 128 bytes.".into());
                return;
            }
        };
        self.app_event_tx.send(AppEvent::PeerGroup {
            thread_id,
            selection,
        });
    }
}
