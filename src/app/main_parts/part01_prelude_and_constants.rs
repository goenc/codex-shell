use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use eframe::egui::{self, Color32, RichText, TextEdit};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tools::ui_edit::api as ui_tool;

use ui_tool::{
    CODEX_STATE_B, CONFIG_BUILD_ROOT_DIR_BROWSE, CONFIG_SAVE, CONFIG_STARTUP_EXE_1_BROWSE,
    CONFIG_STARTUP_EXE_2_BROWSE, CONFIG_STARTUP_EXE_3_BROWSE, CONFIG_STARTUP_EXE_4_BROWSE,
    INPUT_VOICE_TOGGLE, MODE_PROJECT_DEBUG_RUN, MODE_PROJECT_TARGET_MOVE, NAV_BACK_MAIN, REASONING_HIGH,
    REASONING_MEDIUM, REASONING_XHIGH, UI_EDIT_TOGGLE,
    UI_SETTINGS, is_known_ui_command,
};

const DEFAULT_BUILD_COMMAND: &str = "cargo build";
const MAX_HISTORY: usize = 200;
const FONT_RELATIVE_PATH: &str = "assets/fonts/NotoSansJP-Regular.ttf";
const FONT_OFL_RELATIVE_PATH: &str = "assets/fonts/OFL.txt";
const FONT_SOURCE_RELATIVE_PATH: &str = "assets/fonts/FONT_SOURCE.txt";
const CODEX_CONFIG_PATH: &str = r"C:\Users\gonec\.codex\config.toml";
const CODEX_CONFIG_BACKUP_PATH: &str = r"C:\Users\gonec\.codex\config.toml.bak";
const UI_LIVE_RELATIVE_PATH: &str = "runtime/ui/live/ui.json";
const UI_RELOAD_CHECK_INTERVAL_MS: u64 = 250;
const UI_MAIN_SCREEN_ID: &str = "main";
const UI_SETTINGS_SCREEN_ID: &str = "settings";
const PROJECT_DECLARATION_PREFIX: &str = "プロジェクト宣言_";
const PROJECT_DECLARATION_SUFFIX: &str = ".md";
const UI_BASE_OUTER_MARGIN: f32 = 16.0;
const UI_BASE_COMPONENT_GAP: f32 = 8.0;
const PANEL_HORIZONTAL_PADDING: f32 = 8.0;
const INPUT_ACTION_BUTTON_WIDTH: f32 = 96.0;
const FIXED_INPUT_WIDTH: f32 = 780.0;
const FIXED_WINDOW_WIDTH: f32 = FIXED_INPUT_WIDTH
    + INPUT_ACTION_BUTTON_WIDTH
    + UI_BASE_COMPONENT_GAP
    + UI_BASE_OUTER_MARGIN * 2.0
    + PANEL_HORIZONTAL_PADDING * 2.0;
const FIXED_WINDOW_HEIGHT: f32 = 400.0;
const INPUT_FONT_SIZE: f32 = 15.0;
const FIXED_INPUT_HEIGHT_PADDING: f32 = 12.0;
const INPUT_COMMAND_ID_SALT: &str = "input_command_text_edit";
const VOICE_INPUT_HOTKEY_LABEL: &str = "Ctrl+Alt+Right";

