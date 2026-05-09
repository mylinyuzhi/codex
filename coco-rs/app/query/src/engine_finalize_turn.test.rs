// Tests for `render_teammate_message_wrapper` lived here. The helper
// was deleted alongside the engine-side `Inbox`: teammate messages now
// flow through `CommandQueue` with `QueueOrigin::Coordinator` /
// `QueueOrigin::TaskNotification`, and the drain at
// `helpers::queued_command_to_attachment` applies origin-specific
// framing via `wrap_command_text`. TS parity:
// `getAgentPendingMessageAttachments` (`attachments.ts:1085-1100`)
// also surfaces coordinator messages as `queued_command` attachments,
// not as a separate `<teammate-message>` envelope.
