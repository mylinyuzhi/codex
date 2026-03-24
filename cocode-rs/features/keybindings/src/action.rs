//! Keybinding actions.
//!
//! Each action represents a high-level command that can be triggered by a
//! keybinding. Actions use a `namespace:name` string format (e.g.,
//! `"app:interrupt"`, `"chat:submit"`).

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;

/// Actions that can be triggered by keybindings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    // ========== App ==========
    AppInterrupt,
    AppExit,
    AppToggleTodos,
    AppToggleTranscript,
    AppToggleBrief,
    AppToggleTeammatePreview,
    AppToggleTerminal,
    AppGlobalSearch,
    AppQuickOpen,

    // ========== Chat ==========
    ChatCancel,
    ChatCycleMode,
    ChatModelPicker,
    ChatFastMode,
    ChatThinkingToggle,
    ChatSubmit,
    ChatNewline,
    ChatUndo,
    ChatExternalEditor,
    ChatStash,
    ChatImagePaste,
    ChatKillAgents,

    // ========== History ==========
    HistorySearch,
    HistoryPrevious,
    HistoryNext,

    // ========== Task ==========
    TaskBackground,

    // ========== Confirm/Permission ==========
    ConfirmYes,
    ConfirmNo,
    ConfirmPrevious,
    ConfirmNext,
    ConfirmNextField,
    ConfirmPreviousField,
    ConfirmCycleMode,
    ConfirmToggle,
    ConfirmToggleExplanation,
    PermissionToggleDebug,

    // ========== Autocomplete ==========
    AutocompleteAccept,
    AutocompleteDismiss,
    AutocompletePrevious,
    AutocompleteNext,

    // ========== Select ==========
    SelectNext,
    SelectPrevious,
    SelectAccept,
    SelectCancel,

    // ========== Tabs ==========
    TabsNext,
    TabsPrevious,

    // ========== Attachments ==========
    AttachmentsNext,
    AttachmentsPrevious,
    AttachmentsRemove,
    AttachmentsExit,

    // ========== Footer ==========
    FooterNext,
    FooterPrevious,
    FooterSelect,
    FooterOpenSelected,
    FooterClearSelection,

    // ========== Message Selector ==========
    MessageSelectorNext,
    MessageSelectorPrevious,
    MessageSelectorAccept,
    MessageSelectorCancel,
    /// CC-aligned directional navigation.
    MessageSelectorUp,
    MessageSelectorDown,
    MessageSelectorTop,
    MessageSelectorBottom,
    MessageSelectorSelect,

    // ========== Diff ==========
    DiffAccept,
    DiffReject,
    DiffNext,
    DiffPrevious,
    DiffDismiss,
    DiffPreviousSource,
    DiffNextSource,
    DiffBack,
    DiffViewDetails,
    DiffPreviousFile,
    DiffNextFile,

    // ========== Model Picker ==========
    ModelPickerNext,
    ModelPickerPrevious,
    ModelPickerAccept,
    ModelPickerCancel,
    ModelPickerDecreaseEffort,
    ModelPickerIncreaseEffort,

    // ========== Transcript ==========
    TranscriptScrollUp,
    TranscriptScrollDown,
    TranscriptClose,
    TranscriptToggleShowAll,
    TranscriptExit,

    // ========== History Search ==========
    HistorySearchPrevious,
    HistorySearchNext,
    HistorySearchAccept,
    HistorySearchCancel,
    HistorySearchExecute,

    // ========== Theme ==========
    ThemeNext,
    ThemePrevious,
    ThemeAccept,
    ThemeCancel,
    ThemeToggleSyntaxHighlighting,

    // ========== Help ==========
    HelpClose,
    HelpScrollUp,
    HelpScrollDown,

    // ========== Settings ==========
    SettingsNext,
    SettingsPrevious,
    SettingsToggle,
    SettingsClose,
    SettingsSearch,
    SettingsRetry,

    // ========== Plugin ==========
    PluginNextTab,
    PluginPreviousTab,
    PluginNext,
    PluginPrevious,
    PluginAccept,
    PluginClose,
    PluginToggle,
    PluginInstall,

    // ========== Voice ==========
    VoicePushToTalk,

    // ========== cocode-rs Extensions ==========
    /// Toggle plan mode (Tab key).
    ExtTogglePlanMode,
    /// Cycle thinking level (Ctrl+T).
    ExtCycleThinkingLevel,
    /// Cycle model (Ctrl+M).
    ExtCycleModel,
    /// Show command palette (Ctrl+P).
    ExtShowCommandPalette,
    /// Show session browser (Ctrl+S).
    ExtShowSessionBrowser,
    /// Clear screen (Ctrl+L).
    ExtClearScreen,
    /// Open plan editor (Ctrl+G).
    ExtOpenPlanEditor,
    /// Background all tasks (Ctrl+B).
    ExtBackgroundAllTasks,
    /// Toggle tool collapse (Ctrl+Shift+E).
    ExtToggleToolCollapse,
    /// Toggle system reminders display (Ctrl+Shift+R).
    ExtToggleSystemReminders,
    /// Show rewind selector (Esc Esc).
    ExtShowRewindSelector,
    /// Select all (Ctrl+A).
    ExtSelectAll,
    /// Kill to end of line (Ctrl+K).
    ExtKillToEndOfLine,
    /// Yank (Ctrl+Y).
    ExtYank,
    /// Quit (Ctrl+Q).
    ExtQuit,
    /// Show help (? / F1).
    ExtShowHelp,
    /// Toggle thinking display (Ctrl+Shift+T).
    ExtToggleThinking,
    /// Page up in chat.
    ExtPageUp,
    /// Page down in chat.
    ExtPageDown,
    /// Scroll up in chat.
    ExtScrollUp,
    /// Scroll down in chat.
    ExtScrollDown,
    /// Approve all pending permission requests (Ctrl+A in overlay).
    ExtApproveAll,
    // -- Editing actions --
    ExtDeleteBackward,
    ExtDeleteWordBackward,
    ExtDeleteForward,
    ExtDeleteWordForward,
    ExtCursorLeft,
    ExtCursorRight,
    ExtCursorUp,
    ExtCursorDown,
    ExtCursorHome,
    ExtCursorEnd,
    ExtWordLeft,
    ExtWordRight,
    ExtInsertNewline,

    /// Bind a key to a slash command (e.g., `command:doctor`).
    Command(CommandAction),
}

/// A binding that executes a slash command.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommandAction {
    /// The slash command name (without leading `/`).
    pub name: String,
}

impl Action {
    /// Canonical string representation in `namespace:action` format.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AppInterrupt => "app:interrupt",
            Self::AppExit => "app:exit",
            Self::AppToggleTodos => "app:toggleTodos",
            Self::AppToggleTranscript => "app:toggleTranscript",
            Self::AppToggleBrief => "app:toggleBrief",
            Self::AppToggleTeammatePreview => "app:toggleTeammatePreview",
            Self::AppToggleTerminal => "app:toggleTerminal",
            Self::AppGlobalSearch => "app:globalSearch",
            Self::AppQuickOpen => "app:quickOpen",

            Self::ChatCancel => "chat:cancel",
            Self::ChatCycleMode => "chat:cycleMode",
            Self::ChatModelPicker => "chat:modelPicker",
            Self::ChatFastMode => "chat:fastMode",
            Self::ChatThinkingToggle => "chat:thinkingToggle",
            Self::ChatSubmit => "chat:submit",
            Self::ChatNewline => "chat:newline",
            Self::ChatUndo => "chat:undo",
            Self::ChatExternalEditor => "chat:externalEditor",
            Self::ChatStash => "chat:stash",
            Self::ChatImagePaste => "chat:imagePaste",
            Self::ChatKillAgents => "chat:killAgents",

            Self::HistorySearch => "history:search",
            Self::HistoryPrevious => "history:previous",
            Self::HistoryNext => "history:next",

            Self::TaskBackground => "task:background",

            Self::ConfirmYes => "confirm:yes",
            Self::ConfirmNo => "confirm:no",
            Self::ConfirmPrevious => "confirm:previous",
            Self::ConfirmNext => "confirm:next",
            Self::ConfirmNextField => "confirm:nextField",
            Self::ConfirmPreviousField => "confirm:previousField",
            Self::ConfirmCycleMode => "confirm:cycleMode",
            Self::ConfirmToggle => "confirm:toggle",
            Self::ConfirmToggleExplanation => "confirm:toggleExplanation",
            Self::PermissionToggleDebug => "permission:toggleDebug",

            Self::AutocompleteAccept => "autocomplete:accept",
            Self::AutocompleteDismiss => "autocomplete:dismiss",
            Self::AutocompletePrevious => "autocomplete:previous",
            Self::AutocompleteNext => "autocomplete:next",

            Self::SelectNext => "select:next",
            Self::SelectPrevious => "select:previous",
            Self::SelectAccept => "select:accept",
            Self::SelectCancel => "select:cancel",

            Self::TabsNext => "tabs:next",
            Self::TabsPrevious => "tabs:previous",

            Self::AttachmentsNext => "attachments:next",
            Self::AttachmentsPrevious => "attachments:previous",
            Self::AttachmentsRemove => "attachments:remove",
            Self::AttachmentsExit => "attachments:exit",

            Self::FooterNext => "footer:next",
            Self::FooterPrevious => "footer:previous",
            Self::FooterSelect => "footer:select",
            Self::FooterOpenSelected => "footer:openSelected",
            Self::FooterClearSelection => "footer:clearSelection",

            Self::MessageSelectorNext => "messageSelector:next",
            Self::MessageSelectorPrevious => "messageSelector:previous",
            Self::MessageSelectorAccept => "messageSelector:accept",
            Self::MessageSelectorCancel => "messageSelector:cancel",
            Self::MessageSelectorUp => "messageSelector:up",
            Self::MessageSelectorDown => "messageSelector:down",
            Self::MessageSelectorTop => "messageSelector:top",
            Self::MessageSelectorBottom => "messageSelector:bottom",
            Self::MessageSelectorSelect => "messageSelector:select",

            Self::DiffAccept => "diff:accept",
            Self::DiffReject => "diff:reject",
            Self::DiffNext => "diff:next",
            Self::DiffPrevious => "diff:previous",
            Self::DiffDismiss => "diff:dismiss",
            Self::DiffPreviousSource => "diff:previousSource",
            Self::DiffNextSource => "diff:nextSource",
            Self::DiffBack => "diff:back",
            Self::DiffViewDetails => "diff:viewDetails",
            Self::DiffPreviousFile => "diff:previousFile",
            Self::DiffNextFile => "diff:nextFile",

            Self::ModelPickerNext => "modelPicker:next",
            Self::ModelPickerPrevious => "modelPicker:previous",
            Self::ModelPickerAccept => "modelPicker:accept",
            Self::ModelPickerCancel => "modelPicker:cancel",
            Self::ModelPickerDecreaseEffort => "modelPicker:decreaseEffort",
            Self::ModelPickerIncreaseEffort => "modelPicker:increaseEffort",

            Self::TranscriptScrollUp => "transcript:scrollUp",
            Self::TranscriptScrollDown => "transcript:scrollDown",
            Self::TranscriptClose => "transcript:close",
            Self::TranscriptToggleShowAll => "transcript:toggleShowAll",
            Self::TranscriptExit => "transcript:exit",

            Self::HistorySearchPrevious => "historySearch:previous",
            Self::HistorySearchNext => "historySearch:next",
            Self::HistorySearchAccept => "historySearch:accept",
            Self::HistorySearchCancel => "historySearch:cancel",
            Self::HistorySearchExecute => "historySearch:execute",

            Self::ThemeNext => "theme:next",
            Self::ThemePrevious => "theme:previous",
            Self::ThemeAccept => "theme:accept",
            Self::ThemeCancel => "theme:cancel",
            Self::ThemeToggleSyntaxHighlighting => "theme:toggleSyntaxHighlighting",

            Self::HelpClose => "help:close",
            Self::HelpScrollUp => "help:scrollUp",
            Self::HelpScrollDown => "help:scrollDown",

            Self::SettingsNext => "settings:next",
            Self::SettingsPrevious => "settings:previous",
            Self::SettingsToggle => "settings:toggle",
            Self::SettingsClose => "settings:close",
            Self::SettingsSearch => "settings:search",
            Self::SettingsRetry => "settings:retry",

            Self::PluginNextTab => "plugin:nextTab",
            Self::PluginPreviousTab => "plugin:previousTab",
            Self::PluginNext => "plugin:next",
            Self::PluginPrevious => "plugin:previous",
            Self::PluginAccept => "plugin:accept",
            Self::PluginClose => "plugin:close",
            Self::PluginToggle => "plugin:toggle",
            Self::PluginInstall => "plugin:install",

            Self::VoicePushToTalk => "voice:pushToTalk",

            Self::ExtTogglePlanMode => "ext:togglePlanMode",
            Self::ExtCycleThinkingLevel => "ext:cycleThinkingLevel",
            Self::ExtCycleModel => "ext:cycleModel",
            Self::ExtShowCommandPalette => "ext:showCommandPalette",
            Self::ExtShowSessionBrowser => "ext:showSessionBrowser",
            Self::ExtClearScreen => "ext:clearScreen",
            Self::ExtOpenPlanEditor => "ext:openPlanEditor",
            Self::ExtBackgroundAllTasks => "ext:backgroundAllTasks",
            Self::ExtToggleToolCollapse => "ext:toggleToolCollapse",
            Self::ExtToggleSystemReminders => "ext:toggleSystemReminders",
            Self::ExtShowRewindSelector => "ext:showRewindSelector",
            Self::ExtSelectAll => "ext:selectAll",
            Self::ExtKillToEndOfLine => "ext:killToEndOfLine",
            Self::ExtYank => "ext:yank",
            Self::ExtQuit => "ext:quit",
            Self::ExtShowHelp => "ext:showHelp",
            Self::ExtApproveAll => "ext:approveAll",
            Self::ExtToggleThinking => "ext:toggleThinking",
            Self::ExtPageUp => "ext:pageUp",
            Self::ExtPageDown => "ext:pageDown",
            Self::ExtScrollUp => "ext:scrollUp",
            Self::ExtScrollDown => "ext:scrollDown",
            Self::ExtDeleteBackward => "ext:deleteBackward",
            Self::ExtDeleteWordBackward => "ext:deleteWordBackward",
            Self::ExtDeleteForward => "ext:deleteForward",
            Self::ExtDeleteWordForward => "ext:deleteWordForward",
            Self::ExtCursorLeft => "ext:cursorLeft",
            Self::ExtCursorRight => "ext:cursorRight",
            Self::ExtCursorUp => "ext:cursorUp",
            Self::ExtCursorDown => "ext:cursorDown",
            Self::ExtCursorHome => "ext:cursorHome",
            Self::ExtCursorEnd => "ext:cursorEnd",
            Self::ExtWordLeft => "ext:wordLeft",
            Self::ExtWordRight => "ext:wordRight",
            Self::ExtInsertNewline => "ext:insertNewline",

            Self::Command(_) => "command:*",
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command(cmd) => write!(f, "command:{}", cmd.name),
            other => f.write_str(other.as_str()),
        }
    }
}

/// Parse an action from a `namespace:action` string.
impl FromStr for Action {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        // Handle command:* bindings
        if let Some(name) = s.strip_prefix("command:") {
            return Ok(Self::Command(CommandAction {
                name: name.to_string(),
            }));
        }

        match s {
            "app:interrupt" => Ok(Self::AppInterrupt),
            "app:exit" => Ok(Self::AppExit),
            "app:toggleTodos" => Ok(Self::AppToggleTodos),
            "app:toggleTranscript" => Ok(Self::AppToggleTranscript),
            "app:toggleBrief" => Ok(Self::AppToggleBrief),
            "app:toggleTeammatePreview" => Ok(Self::AppToggleTeammatePreview),
            "app:toggleTerminal" => Ok(Self::AppToggleTerminal),
            "app:globalSearch" => Ok(Self::AppGlobalSearch),
            "app:quickOpen" => Ok(Self::AppQuickOpen),

            "chat:cancel" => Ok(Self::ChatCancel),
            "chat:cycleMode" => Ok(Self::ChatCycleMode),
            "chat:modelPicker" => Ok(Self::ChatModelPicker),
            "chat:fastMode" => Ok(Self::ChatFastMode),
            "chat:thinkingToggle" => Ok(Self::ChatThinkingToggle),
            "chat:submit" => Ok(Self::ChatSubmit),
            "chat:newline" => Ok(Self::ChatNewline),
            "chat:undo" => Ok(Self::ChatUndo),
            "chat:externalEditor" => Ok(Self::ChatExternalEditor),
            "chat:stash" => Ok(Self::ChatStash),
            "chat:imagePaste" => Ok(Self::ChatImagePaste),
            "chat:killAgents" => Ok(Self::ChatKillAgents),

            "history:search" => Ok(Self::HistorySearch),
            "history:previous" => Ok(Self::HistoryPrevious),
            "history:next" => Ok(Self::HistoryNext),

            "task:background" => Ok(Self::TaskBackground),

            "confirm:yes" => Ok(Self::ConfirmYes),
            "confirm:no" => Ok(Self::ConfirmNo),
            "confirm:previous" => Ok(Self::ConfirmPrevious),
            "confirm:next" => Ok(Self::ConfirmNext),
            "confirm:nextField" => Ok(Self::ConfirmNextField),
            "confirm:previousField" => Ok(Self::ConfirmPreviousField),
            "confirm:cycleMode" => Ok(Self::ConfirmCycleMode),
            "confirm:toggle" => Ok(Self::ConfirmToggle),
            "confirm:toggleExplanation" => Ok(Self::ConfirmToggleExplanation),
            "permission:toggleDebug" => Ok(Self::PermissionToggleDebug),

            "autocomplete:accept" => Ok(Self::AutocompleteAccept),
            "autocomplete:dismiss" => Ok(Self::AutocompleteDismiss),
            "autocomplete:previous" => Ok(Self::AutocompletePrevious),
            "autocomplete:next" => Ok(Self::AutocompleteNext),

            "select:next" => Ok(Self::SelectNext),
            "select:previous" => Ok(Self::SelectPrevious),
            "select:accept" => Ok(Self::SelectAccept),
            "select:cancel" => Ok(Self::SelectCancel),

            "tabs:next" => Ok(Self::TabsNext),
            "tabs:previous" => Ok(Self::TabsPrevious),

            "attachments:next" => Ok(Self::AttachmentsNext),
            "attachments:previous" => Ok(Self::AttachmentsPrevious),
            "attachments:remove" => Ok(Self::AttachmentsRemove),
            "attachments:exit" => Ok(Self::AttachmentsExit),

            "footer:next" => Ok(Self::FooterNext),
            "footer:previous" => Ok(Self::FooterPrevious),
            "footer:select" => Ok(Self::FooterSelect),
            "footer:openSelected" => Ok(Self::FooterOpenSelected),
            "footer:clearSelection" => Ok(Self::FooterClearSelection),

            "messageSelector:next" => Ok(Self::MessageSelectorNext),
            "messageSelector:previous" => Ok(Self::MessageSelectorPrevious),
            "messageSelector:accept" => Ok(Self::MessageSelectorAccept),
            "messageSelector:cancel" => Ok(Self::MessageSelectorCancel),
            "messageSelector:up" => Ok(Self::MessageSelectorUp),
            "messageSelector:down" => Ok(Self::MessageSelectorDown),
            "messageSelector:top" => Ok(Self::MessageSelectorTop),
            "messageSelector:bottom" => Ok(Self::MessageSelectorBottom),
            "messageSelector:select" => Ok(Self::MessageSelectorSelect),

            "diff:accept" => Ok(Self::DiffAccept),
            "diff:reject" => Ok(Self::DiffReject),
            "diff:next" => Ok(Self::DiffNext),
            "diff:previous" => Ok(Self::DiffPrevious),
            "diff:dismiss" => Ok(Self::DiffDismiss),
            "diff:previousSource" => Ok(Self::DiffPreviousSource),
            "diff:nextSource" => Ok(Self::DiffNextSource),
            "diff:back" => Ok(Self::DiffBack),
            "diff:viewDetails" => Ok(Self::DiffViewDetails),
            "diff:previousFile" => Ok(Self::DiffPreviousFile),
            "diff:nextFile" => Ok(Self::DiffNextFile),

            "modelPicker:next" => Ok(Self::ModelPickerNext),
            "modelPicker:previous" => Ok(Self::ModelPickerPrevious),
            "modelPicker:accept" => Ok(Self::ModelPickerAccept),
            "modelPicker:cancel" => Ok(Self::ModelPickerCancel),
            "modelPicker:decreaseEffort" => Ok(Self::ModelPickerDecreaseEffort),
            "modelPicker:increaseEffort" => Ok(Self::ModelPickerIncreaseEffort),

            "transcript:scrollUp" => Ok(Self::TranscriptScrollUp),
            "transcript:scrollDown" => Ok(Self::TranscriptScrollDown),
            "transcript:close" => Ok(Self::TranscriptClose),
            "transcript:toggleShowAll" => Ok(Self::TranscriptToggleShowAll),
            "transcript:exit" => Ok(Self::TranscriptExit),

            "historySearch:previous" => Ok(Self::HistorySearchPrevious),
            "historySearch:next" => Ok(Self::HistorySearchNext),
            "historySearch:accept" => Ok(Self::HistorySearchAccept),
            "historySearch:cancel" => Ok(Self::HistorySearchCancel),
            "historySearch:execute" => Ok(Self::HistorySearchExecute),

            "theme:next" => Ok(Self::ThemeNext),
            "theme:previous" => Ok(Self::ThemePrevious),
            "theme:accept" => Ok(Self::ThemeAccept),
            "theme:cancel" => Ok(Self::ThemeCancel),
            "theme:toggleSyntaxHighlighting" => Ok(Self::ThemeToggleSyntaxHighlighting),

            "help:close" => Ok(Self::HelpClose),
            "help:scrollUp" => Ok(Self::HelpScrollUp),
            "help:scrollDown" => Ok(Self::HelpScrollDown),

            "settings:next" => Ok(Self::SettingsNext),
            "settings:previous" => Ok(Self::SettingsPrevious),
            "settings:toggle" => Ok(Self::SettingsToggle),
            "settings:close" => Ok(Self::SettingsClose),
            "settings:search" => Ok(Self::SettingsSearch),
            "settings:retry" => Ok(Self::SettingsRetry),

            "plugin:nextTab" => Ok(Self::PluginNextTab),
            "plugin:previousTab" => Ok(Self::PluginPreviousTab),
            "plugin:next" => Ok(Self::PluginNext),
            "plugin:previous" => Ok(Self::PluginPrevious),
            "plugin:accept" => Ok(Self::PluginAccept),
            "plugin:close" => Ok(Self::PluginClose),
            "plugin:toggle" => Ok(Self::PluginToggle),
            "plugin:install" => Ok(Self::PluginInstall),

            "voice:pushToTalk" => Ok(Self::VoicePushToTalk),

            "ext:togglePlanMode" => Ok(Self::ExtTogglePlanMode),
            "ext:cycleThinkingLevel" => Ok(Self::ExtCycleThinkingLevel),
            "ext:cycleModel" => Ok(Self::ExtCycleModel),
            "ext:showCommandPalette" => Ok(Self::ExtShowCommandPalette),
            "ext:showSessionBrowser" => Ok(Self::ExtShowSessionBrowser),
            "ext:clearScreen" => Ok(Self::ExtClearScreen),
            "ext:openPlanEditor" => Ok(Self::ExtOpenPlanEditor),
            "ext:backgroundAllTasks" => Ok(Self::ExtBackgroundAllTasks),
            "ext:toggleToolCollapse" => Ok(Self::ExtToggleToolCollapse),
            "ext:toggleSystemReminders" => Ok(Self::ExtToggleSystemReminders),
            "ext:showRewindSelector" => Ok(Self::ExtShowRewindSelector),
            "ext:selectAll" => Ok(Self::ExtSelectAll),
            "ext:killToEndOfLine" => Ok(Self::ExtKillToEndOfLine),
            "ext:yank" => Ok(Self::ExtYank),
            "ext:quit" => Ok(Self::ExtQuit),
            "ext:showHelp" => Ok(Self::ExtShowHelp),
            "ext:approveAll" => Ok(Self::ExtApproveAll),
            "ext:toggleThinking" => Ok(Self::ExtToggleThinking),
            "ext:pageUp" => Ok(Self::ExtPageUp),
            "ext:pageDown" => Ok(Self::ExtPageDown),
            "ext:scrollUp" => Ok(Self::ExtScrollUp),
            "ext:scrollDown" => Ok(Self::ExtScrollDown),
            "ext:deleteBackward" => Ok(Self::ExtDeleteBackward),
            "ext:deleteWordBackward" => Ok(Self::ExtDeleteWordBackward),
            "ext:deleteForward" => Ok(Self::ExtDeleteForward),
            "ext:deleteWordForward" => Ok(Self::ExtDeleteWordForward),
            "ext:cursorLeft" => Ok(Self::ExtCursorLeft),
            "ext:cursorRight" => Ok(Self::ExtCursorRight),
            "ext:cursorUp" => Ok(Self::ExtCursorUp),
            "ext:cursorDown" => Ok(Self::ExtCursorDown),
            "ext:cursorHome" => Ok(Self::ExtCursorHome),
            "ext:cursorEnd" => Ok(Self::ExtCursorEnd),
            "ext:wordLeft" => Ok(Self::ExtWordLeft),
            "ext:wordRight" => Ok(Self::ExtWordRight),
            "ext:insertNewline" => Ok(Self::ExtInsertNewline),

            _ => Err(format!("unknown action: {s}")),
        }
    }
}

#[cfg(test)]
#[path = "action.test.rs"]
mod tests;
