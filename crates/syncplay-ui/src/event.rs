//! Events delivered from the background runtime engine to the graphical UI presentation thread.

use syncplay_chat::ChatRecord;
use crate::gui::RoomMember;

/// Events delivered from the background engine to the graphical UI thread.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// New chat message to append to the sidebar chat history.
    NewChatRecord(ChatRecord),
    /// Updated list of watch-party room members.
    RoomMembersUpdated(Vec<RoomMember>),
}
