pub const STATUS_MESSAGE: &str = "status.message";
pub const UI_EDIT_LOCKED_HINT: &str = "ui.edit.locked_hint";
pub const MODE_PROJECT_DEBUG_RUN: &str = "mode.project_debug_run";
pub const MODE_PROJECT_TARGET_MOVE: &str = "mode.project_target_move";
pub const INPUT_SEND: &str = "input.send";
pub const INPUT_VOICE_TOGGLE: &str = "input.voice_toggle";
pub const UI_SETTINGS: &str = "ui.settings";
pub const NAV_BACK_MAIN: &str = "nav.back_main";
pub const UI_EDIT_TOGGLE: &str = "ui.edit.toggle";
pub const REASONING_MEDIUM: &str = "reasoning.medium";
pub const REASONING_HIGH: &str = "reasoning.high";
pub const REASONING_XHIGH: &str = "reasoning.xhigh";
pub const CONFIG_WORKING_DIR: &str = "config.working_dir";
pub const CONFIG_SAVE: &str = "config.save";
 
pub const ALL_UI_COMMANDS: &[&str] = &[
    STATUS_MESSAGE,
    UI_EDIT_LOCKED_HINT,
    MODE_PROJECT_DEBUG_RUN,
    MODE_PROJECT_TARGET_MOVE,
    INPUT_SEND,
    INPUT_VOICE_TOGGLE,
    UI_SETTINGS,
    NAV_BACK_MAIN,
    UI_EDIT_TOGGLE,
    REASONING_MEDIUM,
    REASONING_HIGH,
    REASONING_XHIGH,
    CONFIG_WORKING_DIR,
    CONFIG_SAVE,
];

pub fn is_known_ui_command(command: &str) -> bool {
    ALL_UI_COMMANDS.contains(&command)
}
