use computer_use::RecordingHandle;

use super::RecordingController;
use crate::ai::agent::conversation::AIConversationId;

#[test]
fn records_actions_only_for_the_owning_conversation() {
    let owner = AIConversationId::new();
    let other = AIConversationId::new();
    let mut controller = RecordingController::new();
    controller.finish_start(
        "recording".to_string(),
        owner,
        RecordingHandle::new_test(100, 100).0,
    );

    controller.record_action(other, vec!["other".to_string()]);
    controller.record_action(owner, vec!["owner".to_string()]);

    let (_, _, actions) = controller.take_for_conversation(owner).unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].labels, ["owner"]);
}
