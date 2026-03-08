pub const STATUS_MESSAGE: &str = "status.message";
pub const UI_EDIT_LOCKED_HINT: &str = "ui.edit.locked_hint";
pub const MODE_PROJECT_DEBUG_RUN: &str = "mode.project_debug_run";
pub const MODE_PROJECT_TARGET_MOVE: &str = "mode.project_target_move";
pub const INPUT_SEND: &str = "input.send";
pub const INPUT_VOICE_TOGGLE: &str = "input.voice_toggle";
pub const UI_SETTINGS: &str = "ui.settings";
pub const NAV_BACK_MAIN: &str = "nav.back_main";
pub const UI_EDIT_TOGGLE: &str = "ui.edit.toggle";
pub const REASONING_LOW: &str = "reasoning.low";
pub const REASONING_MEDIUM: &str = "reasoning.medium";
pub const REASONING_HIGH: &str = "reasoning.high";
pub const REASONING_XHIGH: &str = "reasoning.xhigh";
pub const CONFIG_MODEL: &str = "config.model";
pub const CONFIG_MODEL_REASONING_EFFORT: &str = "config.model_reasoning_effort";
pub const CONFIG_WORKING_DIR: &str = "config.working_dir";
pub const CONFIG_WORKING_DIR_BROWSE: &str = "config.working_dir.browse";
pub const CONFIG_AUTO_START_EXE_1: &str = "config.auto_start_exe.1";
pub const CONFIG_AUTO_START_EXE_2: &str = "config.auto_start_exe.2";
pub const CONFIG_AUTO_START_EXE_3: &str = "config.auto_start_exe.3";
pub const CONFIG_AUTO_START_EXE_4: &str = "config.auto_start_exe.4";
pub const CONFIG_AUTO_START_EXE_1_BROWSE: &str = "config.auto_start_exe.1.browse";
pub const CONFIG_AUTO_START_EXE_2_BROWSE: &str = "config.auto_start_exe.2.browse";
pub const CONFIG_AUTO_START_EXE_3_BROWSE: &str = "config.auto_start_exe.3.browse";
pub const CONFIG_AUTO_START_EXE_4_BROWSE: &str = "config.auto_start_exe.4.browse";
pub const CONFIG_CODEX_OUTPUT_LOG_DIR_OPEN: &str = "config.codex_output_log_dir.open";
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
    REASONING_LOW,
    REASONING_MEDIUM,
    REASONING_HIGH,
    REASONING_XHIGH,
    CONFIG_MODEL,
    CONFIG_MODEL_REASONING_EFFORT,
    CONFIG_WORKING_DIR,
    CONFIG_WORKING_DIR_BROWSE,
    CONFIG_AUTO_START_EXE_1,
    CONFIG_AUTO_START_EXE_2,
    CONFIG_AUTO_START_EXE_3,
    CONFIG_AUTO_START_EXE_4,
    CONFIG_AUTO_START_EXE_1_BROWSE,
    CONFIG_AUTO_START_EXE_2_BROWSE,
    CONFIG_AUTO_START_EXE_3_BROWSE,
    CONFIG_AUTO_START_EXE_4_BROWSE,
    CONFIG_CODEX_OUTPUT_LOG_DIR_OPEN,
    CONFIG_SAVE,
];

pub fn is_known_ui_command(command: &str) -> bool {
    ALL_UI_COMMANDS.contains(&command)
}
