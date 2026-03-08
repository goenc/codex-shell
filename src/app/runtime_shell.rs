use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use eframe::egui::{self, Color32, RichText, TextEdit};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    DeleteMenu, DrawMenuBar, EnumWindows, GetSystemMenu, GetWindowThreadProcessId, IsWindowVisible,
    MENU_ITEM_FLAGS, MF_BYCOMMAND, SC_CLOSE,
};
#[cfg(windows)]
use windows::core::BOOL;

use super::process_runtime;
use crate::tools::ui_edit::api as ui_tool;

use ui_tool::{
    CONFIG_SAVE, INPUT_SEND, INPUT_VOICE_TOGGLE, MODE_PROJECT_DEBUG_RUN, MODE_PROJECT_TARGET_MOVE,
    NAV_BACK_MAIN, REASONING_HIGH, REASONING_LOW, REASONING_MEDIUM, REASONING_XHIGH,
    UI_EDIT_TOGGLE, UI_SETTINGS, is_known_ui_command,
};

const MAX_HISTORY: usize = 200;
const FONT_RELATIVE_PATH: &str = "assets/fonts/NotoSansJP-Regular.ttf";
const FONT_OFL_RELATIVE_PATH: &str = "assets/fonts/OFL.txt";
const FONT_SOURCE_RELATIVE_PATH: &str = "assets/fonts/FONT_SOURCE.txt";
const CODEX_CONFIG_PATH: &str = r"C:\Users\gonec\.codex\config.toml";
const CODEX_CONFIG_BACKUP_PATH: &str = r"C:\Users\gonec\.codex\config.toml.bak";
const UI_RUNTIME_RELATIVE_PATH: &str = "runtime/ui/ui.json";
const UI_INIT_RELATIVE_PATH: &str = "runtime/ui/init/ui.json";
pub(crate) const UI_RELOAD_CHECK_INTERVAL_MS: u64 = 250;
pub(crate) const UI_MAIN_SCREEN_ID: &str = "main";
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
const CODEX_OUTPUT_TEXT_EDIT_ID_SALT: &str = "codex_output_text_edit";
const CODEX_OUTPUT_LINE_COUNT: usize = 5;
const CODEX_OUTPUT_EVENT_END_PATH: &str = r"C:\Users\gonec\.codex\runtime\agent_event_end.md";
const CODEX_OUTPUT_RUNTIME_LOG_DIR_RELATIVE_PATH: &str = "runtime/codex_output_logs";
const CODEX_OUTPUT_RELOAD_CHECK_INTERVAL_MS: u64 = 250;
const CODEX_STREAM_BEGIN_MARKER: &str = "__CODEX_STREAM_BEGIN__";
const CODEX_STREAM_END_MARKER: &str = "__CODEX_STREAM_END__";
const CODEX_TURN_SEPARATOR: &str = "--------------------------------------------------------------------------------------------------------------------------------------------------------";
const VOICE_INPUT_HOTKEY_LABEL: &str = "Ctrl+Alt+Right";
const POWERSHELL_EXECUTABLE: &str = "pwsh.exe";
const AUTO_START_SLOT_COUNT: usize = 4;
const MODEL_CANDIDATES: [&str; 3] = ["gpt-5.3-codex", "gpt-5.3-codex-spark", "gpt-5.4"];
const REASONING_EFFORT_CANDIDATES: [&str; 4] = ["low", "medium", "high", "xhigh"];
#[cfg(windows)]
const CREATE_NO_WINDOW_FLAG: u32 = 0x0800_0000;

#[cfg(windows)]
#[derive(Default)]
struct WindowSearchContext {
    target_pid: u32,
    hwnd: HWND,
}

#[cfg(windows)]
unsafe extern "system" fn enum_visible_window_by_pid(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let context = unsafe { &mut *(lparam.0 as *mut WindowSearchContext) };
    let mut window_pid = 0u32;
    unsafe {
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
    }
    if window_pid == context.target_pid && unsafe { IsWindowVisible(hwnd).as_bool() } {
        context.hwnd = hwnd;
        return BOOL(0);
    }
    BOOL(1)
}

#[cfg(windows)]
fn find_visible_window_by_pid(pid: u32) -> Option<HWND> {
    let mut context = WindowSearchContext {
        target_pid: pid,
        ..Default::default()
    };
    unsafe {
        let _ = EnumWindows(
            Some(enum_visible_window_by_pid),
            LPARAM((&mut context as *mut WindowSearchContext) as isize),
        );
    }
    (!context.hwnd.0.is_null()).then_some(context.hwnd)
}

#[cfg(windows)]
fn disable_window_close_button_for_pid(pid: u32) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(2);
    let hwnd = loop {
        if let Some(hwnd) = find_visible_window_by_pid(pid) {
            break hwnd;
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "PowerShellウィンドウが見つからないため閉じるボタンを無効化できませんでした"
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    unsafe {
        let system_menu = GetSystemMenu(hwnd, false);
        if system_menu.0.is_null() {
            return Err(anyhow!("システムメニュー取得に失敗しました"));
        }
        DeleteMenu(system_menu, SC_CLOSE, MENU_ITEM_FLAGS(MF_BYCOMMAND.0))
            .context("閉じるボタンの削除に失敗しました")?;
        DrawMenuBar(hwnd).context("メニューバー更新に失敗しました")?;
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct AppConfig {
    pub(crate) working_dir: String,
    pub(crate) auto_start_exe_1: String,
    pub(crate) auto_start_exe_2: String,
    pub(crate) auto_start_exe_3: String,
    pub(crate) auto_start_exe_4: String,
    pub(crate) main_window_width: f32,
    pub(crate) main_window_height: f32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_string()),
            auto_start_exe_1: String::new(),
            auto_start_exe_2: String::new(),
            auto_start_exe_3: String::new(),
            auto_start_exe_4: String::new(),
            main_window_width: FIXED_WINDOW_WIDTH,
            main_window_height: FIXED_WINDOW_HEIGHT,
        }
    }
}

impl AppConfig {
    fn bound_input_mut(&mut self, command: &str) -> Option<&mut String> {
        match command.trim() {
            ui_tool::CONFIG_WORKING_DIR => Some(&mut self.working_dir),
            ui_tool::CONFIG_AUTO_START_EXE_1 => Some(&mut self.auto_start_exe_1),
            ui_tool::CONFIG_AUTO_START_EXE_2 => Some(&mut self.auto_start_exe_2),
            ui_tool::CONFIG_AUTO_START_EXE_3 => Some(&mut self.auto_start_exe_3),
            ui_tool::CONFIG_AUTO_START_EXE_4 => Some(&mut self.auto_start_exe_4),
            _ => None,
        }
    }

    fn auto_start_path(&self, slot: usize) -> Option<&str> {
        match slot {
            0 => Some(&self.auto_start_exe_1),
            1 => Some(&self.auto_start_exe_2),
            2 => Some(&self.auto_start_exe_3),
            3 => Some(&self.auto_start_exe_4),
            _ => None,
        }
    }

    fn set_auto_start_path(&mut self, slot: usize, path: String) -> bool {
        match slot {
            0 => self.auto_start_exe_1 = path,
            1 => self.auto_start_exe_2 = path,
            2 => self.auto_start_exe_3 = path,
            3 => self.auto_start_exe_4 = path,
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiDefinition {
    pub(crate) version: u32,
    pub(crate) assets: UiAssets,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) objects: Vec<UiObject>,
    pub(crate) screens: Vec<UiScreen>,
}

impl Default for UiDefinition {
    fn default() -> Self {
        Self {
            version: 1,
            assets: UiAssets::default(),
            objects: Vec::new(),
            screens: Vec::new(),
        }
    }
}

impl UiDefinition {
    fn normalize_screens(&mut self) {
        let legacy_objects = std::mem::take(&mut self.objects);
        if self.screens.is_empty() {
            self.screens.push(UiScreen {
                id: UI_MAIN_SCREEN_ID.to_string(),
                objects: legacy_objects,
            });
        } else if !legacy_objects.is_empty() {
            if let Some(main_screen_index) = self.screen_index(UI_MAIN_SCREEN_ID) {
                if self.screens[main_screen_index].objects.is_empty() {
                    self.screens[main_screen_index].objects = legacy_objects;
                } else {
                    self.objects.clear();
                }
            } else {
                self.screens.push(UiScreen {
                    id: UI_MAIN_SCREEN_ID.to_string(),
                    objects: legacy_objects,
                });
            }
        }

        if self.screen(UI_MAIN_SCREEN_ID).is_none() {
            self.screens.push(UiScreen {
                id: UI_MAIN_SCREEN_ID.to_string(),
                objects: Vec::new(),
            });
        }
        self.objects.clear();
    }

    pub(crate) fn screen_ids(&self) -> Vec<String> {
        self.screens
            .iter()
            .map(|screen| screen.id.clone())
            .collect()
    }

    pub(crate) fn screen(&self, screen_id: &str) -> Option<&UiScreen> {
        self.screens.iter().find(|screen| screen.id == screen_id)
    }

    fn screen_index(&self, screen_id: &str) -> Option<usize> {
        self.screens
            .iter()
            .position(|screen| screen.id == screen_id)
    }

    fn screen_mut(&mut self, screen_id: &str) -> Option<&mut UiScreen> {
        self.screens
            .iter_mut()
            .find(|screen| screen.id == screen_id)
    }

    pub(crate) fn object_index_in_screen(&self, screen_id: &str, object_id: &str) -> Option<usize> {
        self.screen(screen_id)?
            .objects
            .iter()
            .position(|object| object.id == object_id)
    }

    pub(crate) fn screen_objects(&self, screen_id: &str) -> Option<&Vec<UiObject>> {
        Some(&self.screen(screen_id)?.objects)
    }

    pub(crate) fn screen_objects_mut(&mut self, screen_id: &str) -> Option<&mut Vec<UiObject>> {
        Some(&mut self.screen_mut(screen_id)?.objects)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiScreen {
    pub(crate) id: String,
    pub(crate) objects: Vec<UiObject>,
}

impl Default for UiScreen {
    fn default() -> Self {
        Self {
            id: String::new(),
            objects: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiAssets {
    pub(crate) base_dir: String,
    pub(crate) images: HashMap<String, String>,
}

impl Default for UiAssets {
    fn default() -> Self {
        Self {
            base_dir: "assets/ui".to_string(),
            images: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiObject {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) object_type: String,
    pub(crate) z_index: i32,
    pub(crate) checked: bool,
    pub(crate) position: UiPosition,
    pub(crate) size: UiSize,
    pub(crate) visible: bool,
    pub(crate) enabled: bool,
    pub(crate) bind: UiBind,
    pub(crate) visual: UiVisual,
}

impl Default for UiObject {
    fn default() -> Self {
        Self {
            id: String::new(),
            object_type: "button".to_string(),
            z_index: 0,
            checked: false,
            position: UiPosition::default(),
            size: UiSize::default(),
            visible: true,
            enabled: true,
            bind: UiBind::default(),
            visual: UiVisual::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiPosition {
    pub(crate) x: f32,
    pub(crate) y: f32,
}

impl Default for UiPosition {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiSize {
    pub(crate) w: f32,
    pub(crate) h: f32,
}

impl Default for UiSize {
    fn default() -> Self {
        Self { w: 120.0, h: 32.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct UiBind {
    pub(crate) command: String,
    pub(crate) group: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiVisual {
    pub(crate) background: UiBackground,
    pub(crate) icon: UiIcon,
    pub(crate) text: UiText,
    pub(crate) states: UiStates,
}

impl Default for UiVisual {
    fn default() -> Self {
        Self {
            background: UiBackground::default(),
            icon: UiIcon::default(),
            text: UiText::default(),
            states: UiStates::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct UiBackground {
    pub(crate) image: String,
    pub(crate) fit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiIcon {
    pub(crate) image: String,
    pub(crate) anchor: String,
    pub(crate) offset: UiPosition,
    pub(crate) size: UiSize,
}

impl Default for UiIcon {
    fn default() -> Self {
        Self {
            image: String::new(),
            anchor: "center".to_string(),
            offset: UiPosition::default(),
            size: UiSize::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct UiText {
    pub(crate) value: String,
    pub(crate) align: String,
    pub(crate) font_size: f32,
    pub(crate) font_family: String,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
}

impl Default for UiText {
    fn default() -> Self {
        Self {
            value: String::new(),
            align: "center".to_string(),
            font_size: 16.0,
            font_family: "noto_sans_jp".to_string(),
            bold: false,
            italic: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct UiStates {
    pub(crate) hover: UiStateVisual,
    pub(crate) pressed: UiStateVisual,
    pub(crate) disabled: UiStateVisual,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct UiStateVisual {
    pub(crate) background: UiBackground,
}

#[derive(Clone, Debug)]
struct ProjectDeclarationEntry {
    name: String,
    path: Option<PathBuf>,
}

struct CodexShellApp {
    config: AppConfig,
    ui_definition: UiDefinition,
    ui_definition_path: PathBuf,
    ui_edit_mode: bool,
    ui_edit_grid_visible: bool,
    ui_has_unsaved_changes: bool,
    ui_current_screen_id: String,
    ui_selected_screen_id: String,
    ui_selected_object_id: String,
    ui_selected_object_ids: Vec<String>,
    selected_model: String,
    selected_reasoning_effort: String,
    input_command: String,
    codex_output_text: String,
    status_message: String,
    history: Vec<String>,
    window_size: egui::Vec2,
    input_area_size: egui::Vec2,
    ui_font_names: Vec<String>,
    resize_enabled: bool,
    voice_input_active: bool,
    pending_input_focus: bool,
    powershell_session: Option<PowerShellSession>,
    powershell_output_rx: Option<Receiver<PowerShellOutputLine>>,
    codex_output_streaming_active: bool,
    codex_output_runtime_log_path: Option<PathBuf>,
    ui_resize_locked_by_save: bool,
    target_project_dir_path: Option<PathBuf>,
    project_declarations: Vec<ProjectDeclarationEntry>,
    project_selected_index: Option<usize>,
    moved_project_highlight_key: Option<String>,
    codex_output_event_last_modified: Option<SystemTime>,
    codex_output_last_reload_check: Instant,
    codex_output_waiting_stderr_body: bool,
    is_codex_running: bool,
}

struct RenderObjCtx<'a> {
    ui: &'a mut egui::Ui,
    object: &'a UiObject,
    object_id: &'a str,
    object_type: &'a str,
    object_command: &'a str,
    object_size: egui::Vec2,
    text_font: &'a egui::FontId,
    controls_enabled: bool,
}

struct PowerShellSession {
    process: Child,
    stdin: ChildStdin,
}

struct PowerShellOutputLine {
    text: String,
    is_stderr: bool,
}

fn spawn_powershell_stream_reader<R>(reader: R, is_stderr: bool, sender: Sender<PowerShellOutputLine>)
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let text = line.trim_end_matches(['\r', '\n']).to_string();
                    if sender.send(PowerShellOutputLine { text, is_stderr }).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    let _ = sender.send(PowerShellOutputLine {
                        text: format!("PowerShell出力読み取り失敗: {err}"),
                        is_stderr: true,
                    });
                    break;
                }
            }
        }
    });
}

impl CodexShellApp {
    fn try_new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
        let (loaded_font, ui_font_names) = apply_required_font(&cc.egui_ctx)
            .context("同梱フォント読み込みに失敗しました。assets/fonts を確認してください")?;
        apply_visual_fix(&cc.egui_ctx);

        let config = load_config().unwrap_or_default();
        let ui_definition_path = ensure_runtime_ui_file()?;
        let ui_definition = load_ui_definition(&ui_definition_path)?;
        let ui_selected_object_id = ui_definition
            .screen_objects(UI_MAIN_SCREEN_ID)
            .and_then(|objects| objects.first())
            .map(|object| object.id.clone())
            .unwrap_or_default();
        let ui_selected_object_ids = if ui_selected_object_id.is_empty() {
            Vec::new()
        } else {
            vec![ui_selected_object_id.clone()]
        };
        let selected_model = load_model();
        let selected_reasoning_effort = load_reasoning_effort();

        let mut app = Self {
            config,
            ui_definition,
            ui_definition_path,
            ui_edit_mode: false,
            ui_edit_grid_visible: true,
            ui_has_unsaved_changes: false,
            ui_current_screen_id: UI_MAIN_SCREEN_ID.to_string(),
            ui_selected_screen_id: UI_MAIN_SCREEN_ID.to_string(),
            ui_selected_object_id,
            ui_selected_object_ids,
            selected_model,
            selected_reasoning_effort,
            input_command: String::new(),
            codex_output_text: String::new(),
            status_message: "待機中".to_string(),
            history: Vec::new(),
            window_size: egui::vec2(0.0, 0.0),
            input_area_size: egui::vec2(0.0, 0.0),
            ui_font_names,
            resize_enabled: true,
            voice_input_active: false,
            pending_input_focus: false,
            powershell_session: None,
            powershell_output_rx: None,
            codex_output_streaming_active: false,
            codex_output_runtime_log_path: None,
            ui_resize_locked_by_save: false,
            target_project_dir_path: None,
            project_declarations: Vec::new(),
            project_selected_index: None,
            moved_project_highlight_key: None,
            codex_output_event_last_modified: None,
            codex_output_last_reload_check: Instant::now(),
            codex_output_waiting_stderr_body: false,
            is_codex_running: false,
        };

        app.push_history(format!(
            "同梱フォントを読み込みました: {}",
            loaded_font.display()
        ));
        app.push_history(format!(
            "UI定義を読み込みました: {}",
            app.ui_definition_path.display()
        ));
        app.launch_configured_auto_start_executables();
        app.refresh_project_declarations();
        app.start_powershell_session();
        app.reload_codex_output_from_event_end_file(true);
        app.save_config();
        Ok(app)
    }

    fn save_config(&mut self) {
        match save_config(&self.config) {
            Ok(()) => self.push_history("設定を保存しました"),
            Err(err) => {
                self.update_status(format!("設定保存失敗: {err}"));
                self.push_history(format!("設定保存に失敗しました: {err}"));
            }
        }
    }

    fn apply_window_resize_policy(&mut self, ctx: &egui::Context) {
        let allow_resize = self.ui_edit_mode && !self.ui_resize_locked_by_save;
        if self.resize_enabled == allow_resize {
            return;
        }
        self.resize_enabled = allow_resize;

        if allow_resize {
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(10.0, 10.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(
                8192.0, 8192.0,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
        } else {
            let lock_size = egui::vec2(
                self.config.main_window_width.max(100.0),
                self.config.main_window_height.max(100.0),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(lock_size));
            ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(lock_size));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(lock_size));
        }
    }

    fn push_history(&mut self, message: impl Into<String>) {
        let timestamp = unix_timestamp();
        self.history
            .push(format!("[{timestamp}] {}", message.into().trim()));
        if self.history.len() > MAX_HISTORY {
            let excess = self.history.len() - MAX_HISTORY;
            self.history.drain(0..excess);
        }
    }

    fn update_status(&mut self, message: impl Into<String>) {
        self.status_message = message.into();
    }

    fn runtime_background_color(&self) -> Color32 {
        Color32::from_rgb(224, 224, 224)
    }

    fn apply_runtime_background(&self, ctx: &egui::Context) {
        let panel_bg = self.runtime_background_color();
        ctx.style_mut_of(egui::Theme::Light, |style| {
            style.visuals.panel_fill = panel_bg;
            style.visuals.faint_bg_color = panel_bg;
            style.visuals.extreme_bg_color = panel_bg;
        });
    }

    fn send_input_command_by_button(&mut self) -> bool {
        let input = self.input_command.clone();
        let command = format!(
            "$prompt = @\"\n{input}\n\"@\nWrite-Output \"{CODEX_STREAM_BEGIN_MARKER}\"\ncodex exec $prompt\nWrite-Output \"{CODEX_STREAM_END_MARKER}\"\n"
        );
        let sent = self.send_text_to_powershell(&command);
        if sent {
            self.input_command.clear();
        }
        self.pending_input_focus = true;
        sent
    }

    fn start_powershell_session(&mut self) {
        if self.powershell_session.is_some() {
            return;
        }

        let mut command = Command::new(POWERSHELL_EXECUTABLE);
        command
            .arg("-NoLogo")
            .arg("-NoExit")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            command.creation_flags(CREATE_NO_WINDOW_FLAG);
        }

        match command.spawn() {
            Ok(mut child) => {
                let Some(stdin) = child.stdin.take() else {
                    let _ = child.kill();
                    self.update_status("PowerShell起動失敗: stdin取得不可");
                    self.push_history("PowerShell起動失敗: stdin取得不可");
                    return;
                };
                let Some(stdout) = child.stdout.take() else {
                    let _ = child.kill();
                    self.update_status("PowerShell起動失敗: stdout取得不可");
                    self.push_history("PowerShell起動失敗: stdout取得不可");
                    return;
                };
                let Some(stderr) = child.stderr.take() else {
                    let _ = child.kill();
                    self.update_status("PowerShell起動失敗: stderr取得不可");
                    self.push_history("PowerShell起動失敗: stderr取得不可");
                    return;
                };
                let (tx, rx) = mpsc::channel();
                spawn_powershell_stream_reader(stdout, false, tx.clone());
                spawn_powershell_stream_reader(stderr, true, tx);
                #[cfg(windows)]
                if find_visible_window_by_pid(child.id()).is_some()
                    && let Err(err) = disable_window_close_button_for_pid(child.id())
                {
                    self.push_history(format!("PowerShell閉じるボタン無効化失敗: {err}"));
                }
                self.powershell_session = Some(PowerShellSession {
                    process: child,
                    stdin,
                });
                self.powershell_output_rx = Some(rx);
                self.codex_output_streaming_active = false;
                self.codex_output_runtime_log_path = None;
                self.update_status("PowerShellを起動しました");
                self.push_history("PowerShellを起動しました");
            }
            Err(err) => {
                self.update_status(format!("PowerShell起動失敗: {err}"));
                self.push_history(format!("PowerShell起動失敗: {err}"));
            }
        }
    }

    fn refresh_powershell_session(&mut self) {
        let mut exited_message = None;
        if let Some(session) = self.powershell_session.as_mut() {
            match session.process.try_wait() {
                Ok(Some(status)) => {
                    exited_message = Some(format!("PowerShell終了検出: {status}"));
                }
                Ok(None) => {}
                Err(err) => {
                    exited_message = Some(format!("PowerShell状態確認失敗: {err}"));
                }
            }
        }
        if let Some(message) = exited_message {
            self.powershell_session = None;
            self.powershell_output_rx = None;
            self.codex_output_streaming_active = false;
            self.codex_output_runtime_log_path = None;
            self.clear_codex_running_state();
            self.update_status(message.clone());
            self.push_history(message);
        }
    }

    fn set_codex_running_state(&mut self, running: bool) {
        self.is_codex_running = running;
    }

    fn clear_codex_running_state(&mut self) {
        self.is_codex_running = false;
    }

    fn start_codex_output_runtime_log(&mut self) {
        match create_codex_output_runtime_log_path() {
            Ok(path) => {
                self.codex_output_runtime_log_path = Some(path);
            }
            Err(err) => {
                self.codex_output_runtime_log_path = None;
                self.push_history(format!("Codex出力ログ初期化失敗: {err}"));
            }
        }
    }

    fn append_codex_output_runtime_log_line(&mut self, line: &str) {
        let Some(path) = self.codex_output_runtime_log_path.as_ref() else {
            return;
        };
        let mut file = match fs::OpenOptions::new().create(true).append(true).open(path) {
            Ok(file) => file,
            Err(err) => {
                self.codex_output_runtime_log_path = None;
                self.push_history(format!("Codex出力ログ追記失敗: {err}"));
                return;
            }
        };
        if let Err(err) = writeln!(file, "{line}") {
            self.codex_output_runtime_log_path = None;
            self.push_history(format!("Codex出力ログ書き込み失敗: {err}"));
        }
    }

    fn drain_powershell_output(&mut self) {
        let mut lines = Vec::new();
        if let Some(rx) = self.powershell_output_rx.as_ref() {
            while let Ok(line) = rx.try_recv() {
                lines.push(line);
            }
        }

        for line in lines {
            let trimmed = line.text.trim();
            if trimmed == CODEX_STREAM_BEGIN_MARKER {
                self.codex_output_streaming_active = true;
                self.codex_output_waiting_stderr_body = false;
                self.codex_output_text.clear();
                self.start_codex_output_runtime_log();
                self.codex_output_text.push_str(CODEX_TURN_SEPARATOR);
                self.append_codex_output_runtime_log_line(CODEX_TURN_SEPARATOR);
                continue;
            }
            if trimmed == CODEX_STREAM_END_MARKER {
                self.codex_output_waiting_stderr_body = false;
                if !self.codex_output_text.is_empty() {
                    self.codex_output_text.push('\n');
                }
                self.codex_output_text.push_str(CODEX_TURN_SEPARATOR);
                self.append_codex_output_runtime_log_line(CODEX_TURN_SEPARATOR);
                self.codex_output_streaming_active = false;
                self.codex_output_runtime_log_path = None;
                self.clear_codex_running_state();
                continue;
            }
            if !self.codex_output_streaming_active {
                continue;
            }
            if trimmed.is_empty() || is_codex_output_noise_line(trimmed) {
                continue;
            }
            let output_line = if line.is_stderr {
                let marker = trimmed.strip_prefix("stderr:").unwrap_or(trimmed).trim();
                let lowered_marker = marker.to_ascii_lowercase();
                if lowered_marker == "user" || lowered_marker == "codex" {
                    self.codex_output_waiting_stderr_body = true;
                    continue;
                }
                if self.codex_output_waiting_stderr_body {
                    self.codex_output_waiting_stderr_body = false;
                    marker.to_string()
                } else {
                    continue;
                }
            } else {
                trimmed.to_string()
            };
            if !self.codex_output_text.is_empty() {
                self.codex_output_text.push('\n');
            }
            self.codex_output_text.push_str(&output_line);
            self.append_codex_output_runtime_log_line(&output_line);
        }
    }

    fn send_text_to_powershell(&mut self, text: &str) -> bool {
        if self.powershell_session.is_none() {
            self.update_status("PowerShell未起動のため送信しません");
            self.push_history("PowerShell未起動のため送信しません");
            return false;
        }

        let mut clear_session = false;
        let mut status_message = None;
        let send_result = {
            let session = self
                .powershell_session
                .as_mut()
                .expect("powershell_session existence checked");

            match session.process.try_wait() {
                Ok(Some(status)) => {
                    clear_session = true;
                    status_message =
                        Some(format!("PowerShell終了済みのため送信しません: {status}"));
                    Err(())
                }
                Ok(None) => {
                    if let Err(err) = session.stdin.write_all(text.as_bytes()) {
                        clear_session = true;
                        status_message = Some(format!("PowerShell送信失敗: {err}"));
                        Err(())
                    } else if let Err(err) = session.stdin.write_all(b"\n") {
                        clear_session = true;
                        status_message = Some(format!("PowerShell改行送信失敗: {err}"));
                        Err(())
                    } else if let Err(err) = session.stdin.flush() {
                        clear_session = true;
                        status_message = Some(format!("PowerShell送信反映失敗: {err}"));
                        Err(())
                    } else {
                        Ok(())
                    }
                }
                Err(err) => {
                    clear_session = true;
                    status_message = Some(format!("PowerShell状態確認失敗: {err}"));
                    Err(())
                }
            }
        };

        if clear_session {
            self.powershell_session = None;
            self.clear_codex_running_state();
        }
        if let Some(message) = status_message {
            self.update_status(message.clone());
            self.push_history(message);
        }
        send_result.is_ok()
    }
}

impl CodexShellApp {
    fn project_entry_highlight_key(entry: &ProjectDeclarationEntry) -> String {
        if let Some(path) = entry.path.as_ref() {
            process_runtime::normalize_path_for_dedup(path)
        } else {
            entry.name.trim().to_ascii_lowercase()
        }
    }

    fn selected_project_highlight_key(&self) -> Option<String> {
        self.project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .map(Self::project_entry_highlight_key)
    }

    fn is_selected_project_highlighted(&self) -> bool {
        self.selected_project_highlight_key()
            .is_some_and(|key| self.moved_project_highlight_key.as_deref() == Some(key.as_str()))
    }

    fn is_project_launch_ready(&self) -> bool {
        self.is_selected_project_highlighted()
    }

    fn sync_selected_project_target_dir(&mut self) {
        self.target_project_dir_path = self
            .project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .and_then(|entry| entry.path.as_ref())
            .and_then(|path| path.parent().map(Path::to_path_buf));
    }

    fn selected_project_declaration_path(&self) -> Option<PathBuf> {
        self.project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .and_then(|entry| entry.path.as_ref())
            .cloned()
    }

    fn selected_project_dir_path(&self) -> Option<PathBuf> {
        self.project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .and_then(|entry| entry.path.as_ref())
            .and_then(|path| path.parent().map(Path::to_path_buf))
    }

    fn sync_selected_project_path_bridge_file(&mut self) {
        let Some(project_dir_path) = self.selected_project_dir_path() else {
            return;
        };
        let target_file_path = match selected_repo_bridge_file_path(&self.config.auto_start_exe_1) {
            Ok(path) => path,
            Err(err) => {
                self.push_history(format!(
                    "selected_repo_path.txt の保存先を解決できないため連携をスキップしました: {err}"
                ));
                return;
            }
        };
        if let Some(parent) = target_file_path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            self.update_status(format!("連携先ディレクトリ作成失敗: {err}"));
            self.push_history(format!(
                "selected_repo_path.txt の親ディレクトリ作成に失敗しました: {} ({err})",
                parent.display()
            ));
            return;
        }
        let body = project_dir_path.to_string_lossy().into_owned();
        if let Err(err) = fs::write(&target_file_path, body.as_bytes()) {
            self.update_status(format!("連携ファイル保存失敗: {err}"));
            self.push_history(format!(
                "selected_repo_path.txt の保存に失敗しました: {} ({err})",
                target_file_path.display()
            ));
            return;
        }
        self.push_history(format!(
            "selected_repo_path.txt を更新しました: {}",
            target_file_path.display()
        ));
    }

    fn refresh_project_declarations(&mut self) {
        let base = self.config.working_dir.trim();
        if base.is_empty() {
            self.project_declarations.clear();
            self.project_selected_index = None;
            return;
        }
        let Ok(files) = find_project_declaration_files(Path::new(base)) else {
            self.project_declarations.clear();
            self.project_selected_index = None;
            return;
        };
        let selected_path = self
            .project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .map(|entry| entry.path.clone());
        let mut entries = Vec::new();
        for path in files {
            let name = read_project_name_from_declaration(&path).unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or("Unnamed Project")
                    .to_string()
            });
            entries.push(ProjectDeclarationEntry {
                name,
                path: Some(path),
            });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name).then(left.path.cmp(&right.path)));
        self.project_declarations = entries;
        self.project_selected_index = match selected_path {
            Some(path) => self
                .project_declarations
                .iter()
                .position(|entry| entry.path == path)
                .or_else(|| (!self.project_declarations.is_empty()).then_some(0)),
            None => (!self.project_declarations.is_empty()).then_some(0),
        };
        self.sync_selected_project_target_dir();
    }

    fn launch_active_project_debug_executable(&mut self) {
        if !self.is_project_launch_ready() {
            self.update_status(
                "緑ハイライトのプロジェクトが未選択のためデバッグEXEを起動できません",
            );
            self.push_history("デバッグEXE起動を中止しました: 緑ハイライト未選択");
            return;
        }
        let Some(declaration_path) = self.selected_project_declaration_path() else {
            self.update_status("プロジェクト未選択のためデバッグEXEを起動できません");
            self.push_history("デバッグEXE起動を中止しました: プロジェクト未選択");
            return;
        };
        let exe_path = match resolve_project_debug_executable_path(&declaration_path) {
            Ok(path) => path,
            Err(err) => {
                self.update_status(format!("デバッグEXE解決に失敗: {err}"));
                self.push_history(format!(
                    "デバッグEXE解決に失敗しました: {} ({err})",
                    declaration_path.display()
                ));
                return;
            }
        };
        let exe_text = exe_path.to_string_lossy().into_owned();
        match process_runtime::terminate_running_executable(&exe_text) {
            Ok(killed) => {
                if killed > 0 {
                    self.push_history(format!(
                        "デバッグEXEの既存プロセスを停止しました 件数={killed}: {}",
                        exe_path.display()
                    ));
                }
            }
            Err(err) => {
                self.update_status(format!("デバッグEXE停止失敗: {err}"));
                self.push_history(format!(
                    "デバッグEXEの既存プロセス停止に失敗したため起動を中止しました: {} ({err})",
                    exe_path.display()
                ));
                return;
            }
        }
        let project_dir = declaration_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| {
                exe_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf()
            });
        let launch_target = resolve_project_debug_launch_target(&exe_path);
        match launch_target_with_shell(&launch_target, &project_dir) {
            Ok(()) => {
                self.update_status("デバッグEXEを起動しました");
                self.push_history(format!(
                    "デバッグEXEをシェル起動しました: {} (target: {})",
                    exe_path.display(),
                    launch_target.display()
                ));
            }
            Err(err) => {
                self.update_status(format!("デバッグEXE起動失敗: {err}"));
                self.push_history(format!(
                    "デバッグEXEの起動に失敗しました: {} ({err})",
                    launch_target.display()
                ));
            }
        }
    }

    fn active_project_debug_modified_hhmm(&self) -> Option<String> {
        if !self.is_project_launch_ready() {
            return None;
        }
        let declaration_path = self.selected_project_declaration_path()?;
        let exe_path = resolve_project_debug_executable_path(&declaration_path).ok()?;
        let modified = fs::metadata(exe_path).ok()?.modified().ok()?;
        format_system_time_hhmm(modified)
    }

    fn move_both_shells_to_selected_project_dir(&mut self) {
        let Some(target_dir) = self.target_project_dir_path.clone() else {
            self.update_status("移動対象のプロジェクトフォルダが未選択です");
            self.push_history("プロジェクトフォルダ移動を中止しました: 未選択");
            return;
        };
        let target_dir_text = target_dir.display().to_string();
        let escaped = target_dir_text.replace('"', "\"\"");
        let command = format!("cd \"{escaped}\"");
        if self.send_text_to_powershell(&command) {
            self.moved_project_highlight_key = self.selected_project_highlight_key();
            self.sync_selected_project_path_bridge_file();
            self.update_status(format!(
                "PowerShell作業フォルダを移動しました: {target_dir_text}"
            ));
            self.push_history(format!(
                "PowerShell作業フォルダ移動コマンド送信: {target_dir_text}"
            ));
        } else {
            self.moved_project_highlight_key = None;
        }
    }

    fn toggle_voice_input(&mut self) {
        self.pending_input_focus = true;
        match send_voice_input_hotkey() {
            Ok(()) => {
                self.voice_input_active = !self.voice_input_active;
                self.update_status(format!(
                    "音声入力ホットキー実行済み: {VOICE_INPUT_HOTKEY_LABEL}"
                ));
                self.push_history(format!(
                    "音声入力ホットキー実行: {} -> {}",
                    VOICE_INPUT_HOTKEY_LABEL,
                    if self.voice_input_active {
                        "読み取り中"
                    } else {
                        "音声入力"
                    }
                ));
            }
            Err(err) => {
                self.update_status(format!("音声入力ホットキー実行失敗: {err}"));
                self.push_history(format!("音声入力ホットキー実行失敗: {err}"));
            }
        }
    }

    fn handle_auto_start_exe_browse(&mut self, slot: usize) {
        match process_runtime::select_executable_file_path() {
            Ok(Some(path)) => {
                if self.config.set_auto_start_path(slot, path.clone()) {
                    self.update_status(format!("自動起動設定{}を更新しました", slot + 1));
                    self.push_history(format!("自動起動設定{}を更新: {}", slot + 1, path));
                }
            }
            Ok(None) => {
                self.update_status(format!(
                    "自動起動設定{}の選択をキャンセルしました",
                    slot + 1
                ));
                self.push_history(format!("自動起動設定{}の参照選択をキャンセル", slot + 1));
            }
            Err(err) => {
                self.update_status(format!("自動起動設定{}の参照に失敗: {err}", slot + 1));
                self.push_history(format!("自動起動設定{}の参照失敗: {err}", slot + 1));
            }
        }
    }

    fn launch_configured_auto_start_executables(&mut self) {
        for slot in 0..AUTO_START_SLOT_COUNT {
            let Some(raw_path) = self.config.auto_start_path(slot) else {
                continue;
            };
            let trimmed = raw_path.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            let path = Path::new(&trimmed);
            if !is_valid_auto_start_executable_path(path) {
                self.update_status(format!(
                    "自動起動設定{}をスキップしました: 無効なEXEパスです",
                    slot + 1
                ));
                self.push_history(format!(
                    "自動起動設定{}をスキップ: 無効なEXEパス {}",
                    slot + 1,
                    path.display()
                ));
                continue;
            }
            match try_spawn_auto_start_executable(path) {
                Ok(()) => {
                    self.update_status(format!("自動起動設定{}を起動しました", slot + 1));
                    self.push_history(format!(
                        "自動起動設定{}を起動しました: {}",
                        slot + 1,
                        path.display()
                    ));
                }
                Err(err) => {
                    self.update_status(format!("自動起動設定{}の起動失敗: {err}", slot + 1));
                    self.push_history(format!(
                        "自動起動設定{}の起動失敗: {} ({err})",
                        slot + 1,
                        path.display()
                    ));
                }
            }
        }
    }
}

impl CodexShellApp {
    fn save_ui_definition_cache(&mut self, summary: &str) {
        match save_ui_definition(&self.ui_definition_path, &self.ui_definition) {
            Ok(()) => {
                self.ui_has_unsaved_changes = false;
                self.push_history(summary);
            }
            Err(err) => {
                self.update_status(format!("UI定義保存失敗: {err}"));
                self.push_history(format!("UI定義保存に失敗しました: {err}"));
            }
        }
    }

    fn mark_ui_definition_dirty(&mut self) {
        self.ui_has_unsaved_changes = true;
    }

    fn is_project_launch_command(command: &str) -> bool {
        matches!(command.trim(), ui_tool::MODE_PROJECT_DEBUG_RUN)
    }

    fn is_bind_command_enabled(&self, command: &str) -> bool {
        let command = command.trim();
        if self.is_codex_running
            && matches!(
                command,
                ui_tool::INPUT_SEND
                    | ui_tool::MODE_PROJECT_DEBUG_RUN
                    | ui_tool::MODE_PROJECT_TARGET_MOVE
                    | ui_tool::CONFIG_MODEL
                    | ui_tool::CONFIG_MODEL_REASONING_EFFORT
            )
        {
            return false;
        }
        if Self::is_project_launch_command(command) && !self.is_project_launch_ready() {
            return false;
        }
        match command {
            ui_tool::INPUT_SEND => self.is_selected_project_highlighted() && !self.is_codex_running,
            ui_tool::MODE_PROJECT_DEBUG_RUN => self
                .selected_project_declaration_path()
                .is_some_and(|declaration_path| {
                    resolve_project_debug_executable_path(&declaration_path).is_ok()
                }),
            ui_tool::MODE_PROJECT_TARGET_MOVE => self.target_project_dir_path.is_some(),
            _ => true,
        }
    }

    fn runtime_checked_for_command(&self, command: &str) -> Option<bool> {
        match command.trim() {
            ui_tool::UI_EDIT_TOGGLE => Some(self.ui_edit_mode),
            ui_tool::REASONING_LOW => Some(self.selected_reasoning_effort == "low"),
            ui_tool::REASONING_MEDIUM => Some(self.selected_reasoning_effort == "medium"),
            ui_tool::REASONING_HIGH => Some(self.selected_reasoning_effort == "high"),
            ui_tool::REASONING_XHIGH => Some(self.selected_reasoning_effort == "xhigh"),
            _ => None,
        }
    }

    fn sync_runtime_bound_states(&mut self) -> bool {
        let mut changed = false;
        let ui_edit_mode = self.ui_edit_mode;
        let selected_reasoning_effort = self.selected_reasoning_effort.clone();
        let Some(objects) = self
            .ui_definition
            .screen_objects_mut(self.ui_current_screen_id.as_str())
        else {
            return false;
        };
        for object in objects {
            let desired = match object.bind.command.trim() {
                ui_tool::UI_EDIT_TOGGLE => Some(ui_edit_mode),
                ui_tool::REASONING_LOW => Some(selected_reasoning_effort == "low"),
                ui_tool::REASONING_MEDIUM => Some(selected_reasoning_effort == "medium"),
                ui_tool::REASONING_HIGH => Some(selected_reasoning_effort == "high"),
                ui_tool::REASONING_XHIGH => Some(selected_reasoning_effort == "xhigh"),
                _ => None,
            };
            if let Some(desired_checked) = desired
                && object.checked != desired_checked
            {
                object.checked = desired_checked;
                changed = true;
            }
        }
        changed
    }

    fn is_radio_object_type(object_type: &str) -> bool {
        matches!(object_type.trim(), "radio" | "radio_button")
    }

    fn radio_group_key(object: &UiObject) -> String {
        let key = object.bind.group.trim();
        if key.is_empty() {
            object.id.clone()
        } else {
            key.to_string()
        }
    }

    fn resolve_object_text(&self, object: &UiObject) -> String {
        match object.bind.command.trim() {
            ui_tool::STATUS_MESSAGE => format!("状態: {}", self.status_message),
            ui_tool::UI_EDIT_LOCKED_HINT => "編集モード中のため操作は無効".to_string(),
            ui_tool::INPUT_VOICE_TOGGLE => {
                if self.voice_input_active {
                    "読み取り中".to_string()
                } else if object.visual.text.value.trim().is_empty() {
                    "音声入力".to_string()
                } else {
                    object.visual.text.value.clone()
                }
            }
            _ => {
                if object.visual.text.value.trim().is_empty() {
                    if object.id.trim().is_empty() {
                        object.object_type.clone()
                    } else {
                        object.id.clone()
                    }
                } else {
                    object.visual.text.value.clone()
                }
            }
        }
    }

    fn resolve_label_color(&self, object: &UiObject) -> Color32 {
        let _ = object;
        Color32::BLACK
    }

    fn is_object_runtime_visible(&self, object: &UiObject) -> bool {
        if !object.visible {
            return false;
        }
        match object.bind.command.trim() {
            ui_tool::UI_EDIT_LOCKED_HINT => self.ui_edit_mode,
            _ => true,
        }
    }

    fn handle_mode_project_debug_run(&mut self) {
        self.launch_active_project_debug_executable();
    }

    fn handle_mode_project_target_move(&mut self) {
        self.move_both_shells_to_selected_project_dir();
    }

    fn handle_input_send(&mut self) {
        self.set_codex_running_state(true);
        if !self.send_input_command_by_button() {
            self.clear_codex_running_state();
        }
    }

    fn handle_working_dir_browse(&mut self) {
        match process_runtime::select_directory_path() {
            Ok(Some(path)) => {
                self.config.working_dir = path.clone();
                self.update_status("ルートフォルダを更新しました");
                self.push_history(format!("ルートフォルダを更新: {path}"));
            }
            Ok(None) => {
                self.update_status("ルートフォルダの参照選択をキャンセルしました");
                self.push_history("ルートフォルダの参照選択をキャンセル");
            }
            Err(err) => {
                self.update_status(format!("ルートフォルダの参照に失敗: {err}"));
                self.push_history(format!("ルートフォルダの参照失敗: {err}"));
            }
        }
    }

    fn handle_open_codex_output_log_dir(&mut self) {
        let trimmed = self.config.working_dir.trim();
        if trimmed.is_empty() {
            self.update_status("ログフォルダパスが未設定です");
            self.push_history("ログフォルダを開けませんでした: パス未設定");
            return;
        }
        let log_dir = PathBuf::from(trimmed);
        if !log_dir.exists() {
            self.update_status("ログフォルダが存在しません");
            self.push_history(format!(
                "ログフォルダを開けませんでした: 存在しません ({})",
                log_dir.display()
            ));
            return;
        }
        if !log_dir.is_dir() {
            self.update_status("ログフォルダの指定が不正です");
            self.push_history(format!(
                "ログフォルダを開けませんでした: フォルダではありません ({})",
                log_dir.display()
            ));
            return;
        }
        match Command::new("explorer").arg(&log_dir).spawn() {
            Ok(_) => {
                self.update_status("ログフォルダを開きました");
                self.push_history(format!(
                    "ログフォルダを開きました: {}",
                    log_dir.display()
                ));
            }
            Err(err) => {
                self.update_status(format!("ログフォルダを開けません: {err}"));
                self.push_history(format!(
                    "ログフォルダを開けませんでした: {} ({err})",
                    log_dir.display()
                ));
            }
        }
    }

    fn handle_input_voice_toggle(&mut self) {
        self.toggle_voice_input();
    }

    fn handle_ui_settings(&mut self) {
        self.ui_current_screen_id = UI_SETTINGS_SCREEN_ID.to_string();
        if !self.ui_edit_mode {
            self.ui_selected_screen_id = self.ui_current_screen_id.clone();
        }
    }

    fn handle_nav_back_main(&mut self) {
        self.ui_current_screen_id = UI_MAIN_SCREEN_ID.to_string();
        if !self.ui_edit_mode {
            self.ui_selected_screen_id = self.ui_current_screen_id.clone();
        }
        self.refresh_project_declarations();
    }

    fn handle_config_save(&mut self) {
        self.save_config();
    }

    fn handle_model(&mut self, model: &str) {
        if self.selected_model == model {
            return;
        }
        match update_model(model) {
            Ok(()) => {
                self.selected_model = model.to_string();
                self.update_status(format!("モデルを {model} に設定しました"));
                self.push_history(format!("config.toml を更新しました: model = \"{model}\""));
            }
            Err(err) => {
                self.update_status(format!("config.toml 更新失敗: {err}"));
                self.push_history(format!("config.toml 更新失敗: {err}"));
            }
        }
    }

    fn handle_reasoning_effort(&mut self, effort: &str) {
        if self.selected_reasoning_effort == effort {
            return;
        }
        match update_reasoning_effort(effort) {
            Ok(()) => {
                self.selected_reasoning_effort = effort.to_string();
                self.update_status(format!("思考深度を {effort} に設定しました"));
                self.push_history(format!(
                    "config.toml を更新しました: model_reasoning_effort = \"{effort}\""
                ));
            }
            Err(err) => {
                self.update_status(format!("config.toml 更新失敗: {err}"));
                self.push_history(format!("config.toml 更新失敗: {err}"));
            }
        }
    }

    fn handle_ui_edit_toggle(&mut self) {
        self.ui_edit_mode = !self.ui_edit_mode;
        self.update_status(if self.ui_edit_mode {
            "UI編集モードを有効化しました"
        } else {
            "UI編集モードを無効化しました"
        });
        self.push_history(if self.ui_edit_mode {
            "UI編集モードを有効化しました"
        } else {
            "UI編集モードを無効化しました"
        });
        if self.ui_edit_mode {
            self.ui_selected_screen_id = self.ui_current_screen_id.clone();
            self.ui_resize_locked_by_save = false;
        }
        if self.ui_edit_mode
            && (self.ui_selected_object_id.is_empty()
                || self
                    .ui_definition
                    .object_index_in_screen(
                        self.ui_selected_screen_id.as_str(),
                        &self.ui_selected_object_id,
                    )
                    .is_none())
        {
            self.ui_selected_object_id = self
                .ui_definition
                .screen_objects(self.ui_selected_screen_id.as_str())
                .and_then(|objects| objects.first())
                .map(|object| object.id.clone())
                .unwrap_or_default();
        }
        if self.ui_edit_mode {
            let selected_screen_id = self.ui_selected_screen_id.clone();
            self.ensure_selected_objects_valid(selected_screen_id.as_str());
        } else {
            self.ui_selected_object_ids.clear();
        }
    }

    fn handle_unknown_ui_command(&mut self, command: &str) {
        self.update_status(format!("未対応のUIコマンドです: {command}"));
        self.push_history(format!("未対応UIコマンド: {command}"));
    }

    fn dispatch_ui_command(&mut self, command: &str) {
        let command = command.trim();
        #[cfg(debug_assertions)]
        if !command.is_empty() && !is_known_ui_command(command) {
            self.push_history(format!("未知UIコマンドを検出しました: {command}"));
        }

        match command {
            "" => {}
            MODE_PROJECT_DEBUG_RUN => self.handle_mode_project_debug_run(),
            MODE_PROJECT_TARGET_MOVE => self.handle_mode_project_target_move(),
            INPUT_SEND => self.handle_input_send(),
            INPUT_VOICE_TOGGLE => self.handle_input_voice_toggle(),
            UI_SETTINGS => self.handle_ui_settings(),
            NAV_BACK_MAIN => self.handle_nav_back_main(),
            CONFIG_SAVE => self.handle_config_save(),
            ui_tool::CONFIG_WORKING_DIR_BROWSE => self.handle_working_dir_browse(),
            ui_tool::CONFIG_AUTO_START_EXE_1_BROWSE => self.handle_auto_start_exe_browse(0),
            ui_tool::CONFIG_AUTO_START_EXE_2_BROWSE => self.handle_auto_start_exe_browse(1),
            ui_tool::CONFIG_AUTO_START_EXE_3_BROWSE => self.handle_auto_start_exe_browse(2),
            ui_tool::CONFIG_AUTO_START_EXE_4_BROWSE => self.handle_auto_start_exe_browse(3),
            ui_tool::CONFIG_CODEX_OUTPUT_LOG_DIR_OPEN => self.handle_open_codex_output_log_dir(),
            REASONING_LOW => self.handle_reasoning_effort("low"),
            REASONING_MEDIUM => self.handle_reasoning_effort("medium"),
            REASONING_HIGH => self.handle_reasoning_effort("high"),
            REASONING_XHIGH => self.handle_reasoning_effort("xhigh"),
            UI_EDIT_TOGGLE => self.handle_ui_edit_toggle(),
            other => self.handle_unknown_ui_command(other),
        }
    }
}

impl CodexShellApp {
    fn set_primary_selected_object(&mut self, object_id: String) {
        self.ui_selected_object_id = object_id.clone();
        self.ui_selected_object_ids.clear();
        if !object_id.is_empty() {
            self.ui_selected_object_ids.push(object_id);
        }
    }

    fn ensure_selected_objects_valid(&mut self, screen_id: &str) {
        let Some(objects) = self.ui_definition.screen_objects(screen_id) else {
            self.ui_selected_object_id.clear();
            self.ui_selected_object_ids.clear();
            return;
        };

        self.ui_selected_object_ids
            .retain(|selected_id| objects.iter().any(|object| object.id == *selected_id));

        if self.ui_selected_object_id.is_empty()
            || !objects
                .iter()
                .any(|object| object.id == self.ui_selected_object_id)
        {
            if let Some(first_selected_id) = self.ui_selected_object_ids.first() {
                self.ui_selected_object_id = first_selected_id.clone();
            } else {
                self.ui_selected_object_id = objects
                    .first()
                    .map(|object| object.id.clone())
                    .unwrap_or_default();
            }
        }

        if self.ui_selected_object_id.is_empty() {
            self.ui_selected_object_ids.clear();
            return;
        }

        if let Some(primary_position) = self
            .ui_selected_object_ids
            .iter()
            .position(|selected_id| selected_id == &self.ui_selected_object_id)
        {
            if primary_position != 0 {
                let primary_id = self.ui_selected_object_ids.remove(primary_position);
                self.ui_selected_object_ids.insert(0, primary_id);
            }
        } else {
            self.ui_selected_object_ids
                .insert(0, self.ui_selected_object_id.clone());
        }
    }

    fn render_obj_container_or_group(&self, ctx: &mut RenderObjCtx<'_>) {
        let fill = if ctx.object.visual.background.image.trim().is_empty() {
            Color32::from_gray(250)
        } else {
            Color32::from_gray(242)
        };
        egui::Frame::default()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, Color32::BLACK))
            .inner_margin(egui::Margin::same(4))
            .show(ctx.ui, |ui| {
                ui.set_min_size(ctx.object_size);
            });
    }

    fn render_obj_label(&self, ctx: &mut RenderObjCtx<'_>) {
        let text = self.resolve_object_text(ctx.object);
        let main_align = match ctx.object.visual.text.align.trim() {
            "left" => egui::Align::Min,
            "right" => egui::Align::Max,
            _ => egui::Align::Center,
        };
        let mut rich = RichText::new(text)
            .font(ctx.text_font.clone())
            .color(self.resolve_label_color(ctx.object));
        if ctx.object.visual.text.bold {
            rich = rich.strong();
        }
        if ctx.object.visual.text.italic {
            rich = rich.italics();
        }
        ctx.ui.allocate_ui_with_layout(
            ctx.object_size,
            egui::Layout::left_to_right(egui::Align::Center).with_main_align(main_align),
            |ui| {
                ui.add(
                    egui::Label::new(rich)
                        .selectable(false)
                        .sense(egui::Sense::hover()),
                );
            },
        );
    }

    fn render_obj_input(&mut self, ctx: &mut RenderObjCtx<'_>, state_changed: &mut bool) {
        if ctx.object_id == "input_codex_output" {
            self.render_codex_output_view(ctx);
            return;
        }
        let enabled = ctx.controls_enabled && ctx.object.enabled;
        if let Some(bound_input) = self.config.bound_input_mut(ctx.object_command) {
            let response = ctx.ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    [ctx.object_size.x, ctx.object_size.y],
                    TextEdit::singleline(bound_input).return_key(None),
                )
            });
            if response.inner.changed() {
                *state_changed = true;
            }
            return;
        }

        let input_font_id = egui::FontId::monospace(INPUT_FONT_SIZE);
        let row_height = ctx.ui.fonts_mut(|fonts| fonts.row_height(&input_font_id));
        let desired_rows = ((ctx.object_size.y - FIXED_INPUT_HEIGHT_PADDING).max(row_height)
            / row_height)
            .floor()
            .max(1.0) as usize;
        let frame_stroke = if ctx.object_id == "input_command" {
            egui::Stroke::NONE
        } else {
            egui::Stroke::new(1.0, Color32::BLACK)
        };
        let frame_fill = if ctx.object_id == "input_command" {
            Color32::from_gray(242)
        } else {
            Color32::WHITE
        };
        let input_response = egui::Frame::default()
            .fill(frame_fill)
            .stroke(frame_stroke)
            .inner_margin(egui::Margin::same(4))
            .show(ctx.ui, |ui| {
                let input_line_count = if ctx.object_id == "input_command" {
                    self.input_command.chars().filter(|ch| *ch == '\n').count() + 1
                } else {
                    1
                };
                let mut editor = TextEdit::multiline(&mut self.input_command)
                    .id_source(INPUT_COMMAND_ID_SALT)
                    .font(input_font_id)
                    .interactive(enabled)
                    .desired_width(f32::INFINITY)
                    .desired_rows(desired_rows);
                if ctx.object_id == "input_command" {
                    let ime_commit_this_frame = ui.input(|input| {
                        input.events.iter().any(|event| {
                            matches!(event, egui::Event::Ime(egui::ImeEvent::Commit(_)))
                        })
                    });
                    let input_return_key = if ime_commit_this_frame {
                        None
                    } else {
                        Some(egui::KeyboardShortcut::new(
                            egui::Modifiers::NONE,
                            egui::Key::Enter,
                        ))
                    };
                    editor = editor.frame(false).return_key(input_return_key);
                    let visible_height = (ctx.object_size.y - 8.0).max(1.0);
                    let editor_height = ((input_line_count.max(desired_rows) as f32) * row_height
                        + FIXED_INPUT_HEIGHT_PADDING)
                        .max(visible_height);
                    return egui::ScrollArea::vertical()
                        .id_salt("input_command_vertical_scroll")
                        .auto_shrink([false, false])
                        .max_height(visible_height)
                        .show(ui, |ui| {
                            ui.add_sized(
                                [(ctx.object_size.x - 8.0).max(1.0), editor_height],
                                editor,
                            )
                        })
                        .inner;
                }
                ui.add_sized(
                    [
                        (ctx.object_size.x - 8.0).max(1.0),
                        (ctx.object_size.y - 8.0).max(1.0),
                    ],
                    editor,
                )
            });
        if enabled && self.pending_input_focus {
            input_response.inner.request_focus();
            self.pending_input_focus = false;
        }
        self.input_area_size = input_response.response.rect.size();
    }

    fn render_codex_output_view(&mut self, ctx: &mut RenderObjCtx<'_>) {
        let output_font_id = egui::FontId::monospace(INPUT_FONT_SIZE);
        let row_height = ctx.ui.fonts_mut(|fonts| fonts.row_height(&output_font_id));
        let visible_height =
            row_height * CODEX_OUTPUT_LINE_COUNT as f32 + FIXED_INPUT_HEIGHT_PADDING;
        let output_line_count = self
            .codex_output_text
            .chars()
            .filter(|ch| *ch == '\n')
            .count()
            + 1;
        let editor_height = (output_line_count.max(CODEX_OUTPUT_LINE_COUNT) as f32 * row_height)
            + FIXED_INPUT_HEIGHT_PADDING;
        let mut codex_output_display_text =
            decorate_codex_output_display_lines(&self.codex_output_text);

        egui::Frame::default()
            .fill(Color32::from_gray(242))
            .stroke(egui::Stroke::new(1.0, Color32::BLACK))
            .inner_margin(egui::Margin::same(4))
            .show(ctx.ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("codex_output_vertical_scroll")
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .max_height(visible_height)
                    .show(ui, |ui| {
                        ui.add_sized(
                            [
                                (ctx.object_size.x - 8.0).max(1.0),
                                editor_height.max(visible_height),
                            ],
                            TextEdit::multiline(&mut codex_output_display_text)
                                .id_source(CODEX_OUTPUT_TEXT_EDIT_ID_SALT)
                                .font(output_font_id)
                                .interactive(false)
                                .desired_width(f32::INFINITY)
                                .desired_rows(CODEX_OUTPUT_LINE_COUNT)
                                .frame(false),
                        )
                    });
            });
    }

    fn reload_codex_output_from_event_end_file(&mut self, force: bool) {
        if !force
            && self.codex_output_last_reload_check.elapsed()
                < Duration::from_millis(CODEX_OUTPUT_RELOAD_CHECK_INTERVAL_MS)
        {
            return;
        }
        self.codex_output_last_reload_check = Instant::now();
        let path = Path::new(CODEX_OUTPUT_EVENT_END_PATH);
        let Ok(metadata) = fs::metadata(path) else {
            return;
        };
        let Ok(modified) = metadata.modified() else {
            return;
        };
        if !force && self.codex_output_event_last_modified == Some(modified) {
            return;
        }
        let Ok(body) = fs::read_to_string(path) else {
            return;
        };
        self.codex_output_event_last_modified = Some(modified);
        let filtered = body
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| !is_codex_output_noise_line(line))
            .collect::<Vec<_>>()
            .join("\n");
        if !filtered.is_empty() {
            if !self.codex_output_text.is_empty() {
                self.codex_output_text.push('\n');
            }
            self.codex_output_text.push_str(&filtered);
        }
    }

    fn render_obj_image(&self, ctx: &mut RenderObjCtx<'_>) {
        let image_key = ctx.object.visual.background.image.trim();
        let text = if image_key.is_empty() {
            "image".to_string()
        } else {
            format!("image: {image_key}")
        };
        egui::Frame::default()
            .fill(Color32::from_gray(245))
            .stroke(egui::Stroke::new(1.0, Color32::BLACK))
            .inner_margin(egui::Margin::same(4))
            .show(ctx.ui, |ui| {
                ui.set_min_size(ctx.object_size);
                ui.label(RichText::new(text).color(Color32::BLACK));
            });
    }

    fn render_obj_project_combo_box(&mut self, ctx: &mut RenderObjCtx<'_>) {
        let enabled = ctx.controls_enabled
            && ctx.object.enabled
            && self.is_bind_command_enabled(ctx.object_command);
        let highlight_green = self.is_selected_project_highlighted();
        let placeholder_text = ctx.object.visual.text.value.trim();
        let selected_text = self
            .project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| {
                if placeholder_text.is_empty() {
                    "プロジェクトを選択".to_string()
                } else {
                    placeholder_text.to_string()
                }
            });
        let mut selected_index = self.project_selected_index;
        ctx.ui.add_enabled_ui(enabled, |ui| {
            ui.allocate_ui_with_layout(
                ctx.object_size,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.scope(|ui| {
                        let fixed_width = ctx.object_size.x.max(12.0);
                        if !enabled {
                            ui.visuals_mut().override_text_color = Some(Color32::from_gray(140));
                        }
                        if highlight_green {
                            let green = Color32::from_rgb(188, 233, 188);
                            let open_green = Color32::from_rgb(172, 224, 172);
                            let visuals = &mut ui.style_mut().visuals;
                            visuals.widgets.noninteractive.weak_bg_fill = green;
                            visuals.widgets.noninteractive.bg_fill = green;
                            visuals.widgets.inactive.weak_bg_fill = green;
                            visuals.widgets.inactive.bg_fill = green;
                            visuals.widgets.hovered.weak_bg_fill = green;
                            visuals.widgets.hovered.bg_fill = green;
                            visuals.widgets.active.weak_bg_fill = open_green;
                            visuals.widgets.active.bg_fill = open_green;
                            visuals.widgets.open.weak_bg_fill = open_green;
                            visuals.widgets.open.bg_fill = open_green;
                        }
                        ui.style_mut()
                            .text_styles
                            .insert(egui::TextStyle::Button, ctx.text_font.clone());
                        ui.style_mut()
                            .text_styles
                            .insert(egui::TextStyle::Body, ctx.text_font.clone());
                        ui.spacing_mut().combo_width = fixed_width;
                        ui.spacing_mut().interact_size.y = ctx.object_size.y.max(18.0);
                        ui.set_min_width(fixed_width);
                        ui.set_max_width(fixed_width);
                        egui::ComboBox::from_id_salt(("project_combo_box", ctx.object_id))
                            .width(fixed_width)
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                if self.project_declarations.is_empty() {
                                    ui.label("プロジェクト宣言_*.md が見つかりません");
                                } else {
                                    for (index, entry) in
                                        self.project_declarations.iter().enumerate()
                                    {
                                        ui.selectable_value(
                                            &mut selected_index,
                                            Some(index),
                                            entry.name.as_str(),
                                        );
                                    }
                                }
                            });
                    });
                },
            );
        });
        if selected_index != self.project_selected_index {
            self.project_selected_index = selected_index;
            self.sync_selected_project_target_dir();
        }
    }

    fn render_obj_codex_config_combo_box(&mut self, ctx: &mut RenderObjCtx<'_>) {
        let enabled = ctx.controls_enabled
            && ctx.object.enabled
            && self.is_bind_command_enabled(ctx.object_command);
        let fixed_width = ctx.object_size.x.max(12.0);
        let placeholder_text = ctx.object.visual.text.value.trim();
        let id_salt = ("codex_config_combo_box", ctx.object_id);

        match ctx.object_command {
            ui_tool::CONFIG_MODEL => {
                let mut selected_model = self.selected_model.clone();
                let selected_text = if selected_model.is_empty() {
                    if placeholder_text.is_empty() {
                        MODEL_CANDIDATES[0].to_string()
                    } else {
                        placeholder_text.to_string()
                    }
                } else {
                    selected_model.clone()
                };
                ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.allocate_ui_with_layout(
                        ctx.object_size,
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let previous_override = ui.style().visuals.override_text_color;
                            if !enabled {
                                ui.visuals_mut().override_text_color =
                                    Some(Color32::from_gray(140));
                            }
                            ui.style_mut()
                                .text_styles
                                .insert(egui::TextStyle::Button, ctx.text_font.clone());
                            ui.style_mut()
                                .text_styles
                                .insert(egui::TextStyle::Body, ctx.text_font.clone());
                            ui.spacing_mut().combo_width = fixed_width;
                            ui.spacing_mut().interact_size.y = ctx.object_size.y.max(18.0);
                            ui.set_min_width(fixed_width);
                            ui.set_max_width(fixed_width);
                            egui::ComboBox::from_id_salt(id_salt)
                                .width(fixed_width)
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    for model in MODEL_CANDIDATES {
                                        ui.selectable_value(
                                            &mut selected_model,
                                            model.to_string(),
                                            model,
                                        );
                                    }
                                });
                            ui.visuals_mut().override_text_color = previous_override;
                        },
                    );
                });
                if selected_model != self.selected_model {
                    self.handle_model(&selected_model);
                }
            }
            ui_tool::CONFIG_MODEL_REASONING_EFFORT => {
                let mut selected_effort = self.selected_reasoning_effort.clone();
                let selected_text = if selected_effort.is_empty() {
                    if placeholder_text.is_empty() {
                        REASONING_EFFORT_CANDIDATES[1].to_string()
                    } else {
                        placeholder_text.to_string()
                    }
                } else {
                    selected_effort.clone()
                };
                ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.allocate_ui_with_layout(
                        ctx.object_size,
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let previous_override = ui.style().visuals.override_text_color;
                            if !enabled {
                                ui.visuals_mut().override_text_color =
                                    Some(Color32::from_gray(140));
                            }
                            ui.style_mut()
                                .text_styles
                                .insert(egui::TextStyle::Button, ctx.text_font.clone());
                            ui.style_mut()
                                .text_styles
                                .insert(egui::TextStyle::Body, ctx.text_font.clone());
                            ui.spacing_mut().combo_width = fixed_width;
                            ui.spacing_mut().interact_size.y = ctx.object_size.y.max(18.0);
                            ui.set_min_width(fixed_width);
                            ui.set_max_width(fixed_width);
                            egui::ComboBox::from_id_salt(id_salt)
                                .width(fixed_width)
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    for effort in REASONING_EFFORT_CANDIDATES {
                                        ui.selectable_value(
                                            &mut selected_effort,
                                            effort.to_string(),
                                            effort,
                                        );
                                    }
                                });
                            ui.visuals_mut().override_text_color = previous_override;
                        },
                    );
                });
                if selected_effort != self.selected_reasoning_effort {
                    self.handle_reasoning_effort(&selected_effort);
                }
            }
            _ => self.render_obj_project_combo_box(ctx),
        }
    }

    fn render_obj_checkbox(&self, ctx: &mut RenderObjCtx<'_>) -> Option<bool> {
        let text = self.resolve_object_text(ctx.object);
        let enabled = ctx.controls_enabled
            && ctx.object.enabled
            && self.is_bind_command_enabled(ctx.object_command);
        let mut checked = self
            .runtime_checked_for_command(ctx.object_command)
            .unwrap_or(ctx.object.checked);
        let mut rich = RichText::new(text).font(ctx.text_font.clone());
        if ctx.object.visual.text.bold {
            rich = rich.strong();
        }
        if ctx.object.visual.text.italic {
            rich = rich.italics();
        }
        let response = ctx.ui.add_enabled_ui(enabled, |ui| {
            ui.add_sized(
                [ctx.object_size.x, ctx.object_size.y],
                egui::Checkbox::new(&mut checked, rich),
            )
        });
        if response.inner.changed() {
            Some(checked)
        } else {
            None
        }
    }

    fn render_obj_radio(&self, ctx: &mut RenderObjCtx<'_>) -> bool {
        let text = self.resolve_object_text(ctx.object);
        let enabled = ctx.controls_enabled
            && ctx.object.enabled
            && self.is_bind_command_enabled(ctx.object_command);
        let checked = self
            .runtime_checked_for_command(ctx.object_command)
            .unwrap_or(ctx.object.checked);
        let mut rich = RichText::new(text).font(ctx.text_font.clone());
        if ctx.object.visual.text.bold {
            rich = rich.strong();
        }
        if ctx.object.visual.text.italic {
            rich = rich.italics();
        }
        let response = ctx.ui.add_enabled_ui(enabled, |ui| {
            ui.add_sized(
                [ctx.object_size.x, ctx.object_size.y],
                egui::RadioButton::new(checked, rich),
            )
        });
        response.inner.clicked() && !checked
    }

    fn render_obj_button(&self, ctx: &mut RenderObjCtx<'_>) -> bool {
        let text = self.resolve_object_text(ctx.object);
        let disabled_for_selected_project =
            ctx.object_id == "btn_project_target_move" && self.is_selected_project_highlighted();
        let enabled = ctx.controls_enabled
            && ctx.object.enabled
            && self.is_bind_command_enabled(ctx.object_command)
            && !disabled_for_selected_project;
        let mut rich = RichText::new(text).font(ctx.text_font.clone());
        if ctx.object.visual.text.bold {
            rich = rich.strong();
        }
        if ctx.object.visual.text.italic {
            rich = rich.italics();
        }
        if !enabled {
            rich = rich.color(Color32::from_gray(140));
        }
        let response = ctx.ui.scope(|ui| {
            if ctx.object_command == ui_tool::INPUT_SEND && self.is_codex_running {
                let orange = Color32::from_rgb(245, 173, 89);
                let orange_active = Color32::from_rgb(232, 154, 64);
                let visuals = &mut ui.style_mut().visuals;
                visuals.widgets.noninteractive.weak_bg_fill = orange;
                visuals.widgets.noninteractive.bg_fill = orange;
                visuals.widgets.inactive.weak_bg_fill = orange;
                visuals.widgets.inactive.bg_fill = orange;
                visuals.widgets.hovered.weak_bg_fill = orange;
                visuals.widgets.hovered.bg_fill = orange;
                visuals.widgets.active.weak_bg_fill = orange_active;
                visuals.widgets.active.bg_fill = orange_active;
                visuals.widgets.open.weak_bg_fill = orange_active;
                visuals.widgets.open.bg_fill = orange_active;
            }
            ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    [ctx.object_size.x, ctx.object_size.y],
                    egui::Button::new(rich),
                )
            })
        });
        response.inner.inner.clicked()
    }

    fn render_obj_by_type(
        &mut self,
        ctx: &mut RenderObjCtx<'_>,
        state_changed: &mut bool,
        clicked: &mut bool,
        checkbox_changed: &mut Option<bool>,
        radio_selected: &mut bool,
    ) {
        match ctx.object_type {
            "panel" => self.render_obj_container_or_group(ctx),
            "label" => self.render_obj_label(ctx),
            "input" => self.render_obj_input(ctx, state_changed),
            "image" => self.render_obj_image(ctx),
            "combo_box" | "combobox" | "project_dropdown" | "dropdown" => {
                self.render_obj_codex_config_combo_box(ctx)
            }
            "checkbox" => {
                *checkbox_changed = self.render_obj_checkbox(ctx);
            }
            "radio" | "radio_button" => {
                *radio_selected = self.render_obj_radio(ctx);
            }
            _ => {
                *clicked = self.render_obj_button(ctx);
            }
        }
    }
}

impl CodexShellApp {
    fn render_runtime_ui_objects(&mut self, ctx: &egui::Context) {
        let mut clicked_commands = Vec::new();
        let mut position_changed = false;
        let mut state_changed = self.sync_runtime_bound_states();
        let controls_enabled = !self.ui_edit_mode;
        let object_layer_order = egui::Order::Foreground;
        let mut rendered_layers = Vec::new();
        let current_screen_id = self.ui_current_screen_id.clone();
        let Some(screen_snapshot) = self
            .ui_definition
            .screen_objects(current_screen_id.as_str())
            .cloned()
        else {
            return;
        };
        self.ensure_selected_objects_valid(current_screen_id.as_str());
        if self.ui_edit_mode && self.ui_edit_grid_visible {
            self.render_edit_grid(ctx);
        }
        self.render_modal_screen_tint(ctx, current_screen_id.as_str());
        let mut ordered_indices: Vec<usize> = (0..screen_snapshot.len()).collect();
        ordered_indices.sort_by(|left, right| {
            screen_snapshot[*left]
                .z_index
                .cmp(&screen_snapshot[*right].z_index)
                .then(left.cmp(right))
        });

        for index in ordered_indices {
            let object = screen_snapshot[index].clone();
            if !self.is_object_runtime_visible(&object) {
                continue;
            }

            let object_type = object.object_type.trim().to_string();
            let object_id = object.id.clone();
            let object_command = object.bind.command.trim().to_string();
            let object_size = egui::vec2(object.size.w.max(12.0), object.size.h.max(12.0));
            let text_size = object.visual.text.font_size.max(1.0);
            let requested_family = object.visual.text.font_family.trim();
            let text_font = if !requested_family.is_empty()
                && self
                    .ui_font_names
                    .iter()
                    .any(|name| name == requested_family)
            {
                egui::FontId::new(
                    text_size,
                    egui::FontFamily::Name(Arc::from(requested_family.to_string())),
                )
            } else {
                egui::FontId::new(text_size, egui::FontFamily::Proportional)
            };
            let area_interactable = true;
            let mut clicked = false;
            let mut checkbox_changed: Option<bool> = None;
            let mut radio_selected = false;
            let layer_id = egui::LayerId::new(
                object_layer_order,
                egui::Id::new(("ui_object", object_id.clone())),
            );
            rendered_layers.push(layer_id);

            let area_response = egui::Area::new(layer_id.id)
                .order(object_layer_order)
                .interactable(area_interactable)
                .current_pos(egui::pos2(object.position.x, object.position.y))
                .sense(if self.ui_edit_mode {
                    egui::Sense::click_and_drag()
                } else {
                    egui::Sense::hover()
                })
                .show(ctx, |ui| {
                    let mut render_ctx = RenderObjCtx {
                        ui,
                        object: &object,
                        object_id: object_id.as_str(),
                        object_type: object_type.as_str(),
                        object_command: object_command.as_str(),
                        object_size,
                        text_font: &text_font,
                        controls_enabled,
                    };
                    self.render_obj_by_type(
                        &mut render_ctx,
                        &mut state_changed,
                        &mut clicked,
                        &mut checkbox_changed,
                        &mut radio_selected,
                    );
                });

            if object_command == ui_tool::MODE_PROJECT_DEBUG_RUN
                && let Some(modified_hhmm) = self.active_project_debug_modified_hhmm()
            {
                let debug_time_layer = egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new(("debug_button_time", object_id.clone())),
                );
                let painter = ctx.layer_painter(debug_time_layer);
                painter.text(
                    egui::pos2(
                        area_response.response.rect.left(),
                        area_response.response.rect.bottom() + 2.0,
                    ),
                    egui::Align2::LEFT_TOP,
                    format!("更新日時 {modified_hhmm}"),
                    egui::FontId::new(12.0, egui::FontFamily::Proportional),
                    Color32::BLACK,
                );
            }

            let pointer_clicked_on_area = ctx.input(|input| {
                input.pointer.primary_clicked()
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|pos| area_response.response.rect.contains(pos))
            });
            if self.ui_edit_mode
                && (area_response.response.clicked()
                    || area_response.response.drag_started()
                    || pointer_clicked_on_area)
            {
                self.ui_selected_screen_id = current_screen_id.clone();
                let additive_select = ctx.input(|input| {
                    input.modifiers.ctrl || input.modifiers.command || input.modifiers.shift
                });
                if additive_select {
                    if self.ui_selected_object_ids.is_empty()
                        && !self.ui_selected_object_id.is_empty()
                    {
                        self.ui_selected_object_ids
                            .push(self.ui_selected_object_id.clone());
                    }
                    if !self
                        .ui_selected_object_ids
                        .iter()
                        .any(|selected_id| selected_id == &object_id)
                    {
                        self.ui_selected_object_ids.push(object_id.clone());
                    }
                    if self.ui_selected_object_id.is_empty() {
                        self.ui_selected_object_id = object_id.clone();
                    }
                } else {
                    self.set_primary_selected_object(object_id.clone());
                }
            }

            if let Some(next_checked) = checkbox_changed {
                let Some(screen_objects) = self
                    .ui_definition
                    .screen_objects_mut(current_screen_id.as_str())
                else {
                    continue;
                };
                let target = &mut screen_objects[index];
                if target.checked != next_checked {
                    target.checked = next_checked;
                    state_changed = true;
                    if !object_command.is_empty() {
                        clicked_commands.push(object_command.clone());
                    }
                }
            }

            if radio_selected {
                let group_key = Self::radio_group_key(&object);
                let mut group_changed = false;
                let Some(screen_objects) = self
                    .ui_definition
                    .screen_objects_mut(current_screen_id.as_str())
                else {
                    continue;
                };
                for (other_index, other) in screen_objects.iter_mut().enumerate() {
                    if Self::is_radio_object_type(&other.object_type)
                        && Self::radio_group_key(other) == group_key
                    {
                        let next_checked = other_index == index;
                        if other.checked != next_checked {
                            other.checked = next_checked;
                            group_changed = true;
                        }
                    }
                }
                if group_changed {
                    state_changed = true;
                    if !object_command.is_empty() {
                        clicked_commands.push(object_command.clone());
                    }
                }
            }

            if self.ui_edit_mode
                && self
                    .ui_selected_object_ids
                    .iter()
                    .any(|selected_id| selected_id == &object_id)
            {
                let highlight_rect = area_response.response.rect.expand(2.0);
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new(("ui_selected_highlight", object_id.clone())),
                ));
                let is_primary = self.ui_selected_object_id == object_id;
                let (fill_color, stroke_color) = if is_primary {
                    (
                        Color32::from_rgba_unmultiplied(255, 0, 0, 26),
                        Color32::from_rgba_unmultiplied(255, 0, 0, 180),
                    )
                } else {
                    (
                        Color32::from_rgba_unmultiplied(180, 220, 255, 28),
                        Color32::from_rgba_unmultiplied(115, 175, 235, 200),
                    )
                };
                painter.rect(
                    highlight_rect,
                    egui::CornerRadius::same(2),
                    fill_color,
                    egui::Stroke::new(2.0, stroke_color),
                    egui::StrokeKind::Outside,
                );
            }

            if self.ui_edit_mode {
                let drag_delta = area_response.response.drag_delta();
                if drag_delta != egui::Vec2::ZERO
                    && self
                        .ui_selected_object_ids
                        .iter()
                        .any(|selected_id| selected_id == &object_id)
                {
                    let selected_ids = self.ui_selected_object_ids.clone();
                    let Some(screen_objects) = self
                        .ui_definition
                        .screen_objects_mut(current_screen_id.as_str())
                    else {
                        continue;
                    };
                    for target in screen_objects.iter_mut() {
                        if selected_ids
                            .iter()
                            .any(|selected_id| selected_id == &target.id)
                        {
                            target.position.x += drag_delta.x;
                            target.position.y += drag_delta.y;
                            position_changed = true;
                        }
                    }
                }
            }

            if clicked && !object_command.is_empty() {
                clicked_commands.push(object_command);
            }
        }

        if position_changed {
            self.mark_ui_definition_dirty();
        }
        if state_changed {
            // ランタイム同期で変わる checked は即保存しない。
        }
        let popup_open = egui::Popup::is_any_open(ctx);
        if !popup_open {
            ctx.memory_mut(|memory| {
                let areas = memory.areas_mut();
                for layer in rendered_layers {
                    areas.move_to_top(layer);
                }
            });
        }

        if controls_enabled {
            for command in clicked_commands {
                self.dispatch_ui_command(&command);
            }
        }
    }

    fn render_ui_editor(&mut self, ctx: &egui::Context) {
        if !self.ui_edit_mode {
            return;
        }

        let before_screen_id = self.ui_selected_screen_id.clone();
        let before_object_id = self.ui_selected_object_id.clone();
        let events = ui_tool::render_ui_editor_viewport(
            ctx,
            &mut self.ui_definition,
            &mut self.ui_selected_screen_id,
            &mut self.ui_selected_object_id,
            &mut self.ui_selected_object_ids,
            &mut self.ui_edit_grid_visible,
            &self.ui_font_names,
            true,
            self.window_size,
            self.ui_has_unsaved_changes,
        );
        if self.ui_selected_screen_id != before_screen_id {
            self.ui_current_screen_id = self.ui_selected_screen_id.clone();
        }
        if self.ui_selected_screen_id != before_screen_id
            || self.ui_selected_object_id != before_object_id
        {
            self.set_primary_selected_object(self.ui_selected_object_id.clone());
        } else {
            let selected_screen_id = self.ui_selected_screen_id.clone();
            self.ensure_selected_objects_valid(selected_screen_id.as_str());
        }
        if events.changed {
            self.mark_ui_definition_dirty();
        }
        if events.save_requested {
            let current_size = ctx.content_rect().size();
            if current_size.x > 1.0 && current_size.y > 1.0 {
                self.config.main_window_width = current_size.x;
                self.config.main_window_height = current_size.y;
                self.save_config();
                self.ui_resize_locked_by_save = true;
            }
            self.save_ui_definition_cache("UI編集内容を保存しました");
            self.update_status("UI編集内容を保存しました");
        }
        if events.closed {
            self.ui_edit_mode = false;
            self.update_status("UI編集モードを無効化しました");
            self.push_history("UI編集ウィンドウを閉じました");
        }
    }

    fn render_edit_grid(&self, ctx: &egui::Context) {
        let grid_step_px = 10;
        let major_step_px = 50;
        let rect = ctx.content_rect();
        let max_x = rect.right().max(0.0).floor() as i32;
        let max_y = rect.bottom().max(0.0).floor() as i32;
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("ui_edit_grid"),
        ));
        let minor_color = Color32::from_rgba_unmultiplied(190, 160, 220, 70);
        let major_color = Color32::from_rgba_unmultiplied(170, 130, 210, 120);

        let mut x = 0;
        while x <= max_x {
            let is_major = x % major_step_px == 0;
            painter.line_segment(
                [
                    egui::pos2(x as f32, 0.0),
                    egui::pos2(x as f32, max_y as f32),
                ],
                egui::Stroke::new(
                    if is_major { 1.6 } else { 1.0 },
                    if is_major { major_color } else { minor_color },
                ),
            );
            x += grid_step_px;
        }

        let mut y = 0;
        while y <= max_y {
            let is_major = y % major_step_px == 0;
            painter.line_segment(
                [
                    egui::pos2(0.0, y as f32),
                    egui::pos2(max_x as f32, y as f32),
                ],
                egui::Stroke::new(
                    if is_major { 1.6 } else { 1.0 },
                    if is_major { major_color } else { minor_color },
                ),
            );
            y += grid_step_px;
        }
    }

    fn render_modal_screen_tint(&self, ctx: &egui::Context, screen_id: &str) {
        if !Self::is_modal_screen(screen_id) {
            return;
        }
        let content_rect = ctx.content_rect();
        let inset_x = (content_rect.width() * 0.05).max(0.0);
        let inset_y = (content_rect.height() * 0.05).max(0.0);
        let overlay_rect = content_rect.shrink2(egui::vec2(inset_x, inset_y));
        let overlay_layer = egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("runtime_modal_screen_tint"),
        );
        let painter = ctx.layer_painter(overlay_layer);
        painter.rect(
            overlay_rect,
            egui::CornerRadius::ZERO,
            Color32::from_rgba_unmultiplied(196, 170, 224, 42),
            egui::Stroke::NONE,
            egui::StrokeKind::Middle,
        );
    }

    fn is_modal_screen(screen_id: &str) -> bool {
        let normalized = screen_id.trim();
        if normalized == UI_MAIN_SCREEN_ID {
            return false;
        }
        !Self::is_custom_windows_screen(normalized)
    }

    fn is_custom_windows_screen(screen_id: &str) -> bool {
        let normalized = screen_id.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return false;
        }
        let looks_like_window_screen = normalized.contains("window")
            || normalized.starts_with("win_")
            || normalized.ends_with("_win");
        looks_like_window_screen && !normalized.contains("modal")
    }
}

impl eframe::App for CodexShellApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    modifiers,
                    ..
                } if modifiers.ctrl && modifiers.alt
            )
        });
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_window_resize_policy(ctx);
        self.refresh_powershell_session();
        self.drain_powershell_output();
        self.reload_codex_output_from_event_end_file(false);
        let next_window_size = ctx.content_rect().size();
        if self.ui_edit_mode {
            let width_changed = (next_window_size.x - self.window_size.x).abs() >= 1.0;
            let height_changed = (next_window_size.y - self.window_size.y).abs() >= 1.0;
            if self.window_size.x > 1.0
                && self.window_size.y > 1.0
                && (width_changed || height_changed)
            {
                self.mark_ui_definition_dirty();
            }
        }
        self.window_size = next_window_size;
        self.apply_runtime_background(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.allocate_space(egui::Vec2::ZERO);
        });
        self.render_runtime_ui_objects(ctx);

        self.render_ui_editor(ctx);
        ctx.request_repaint_after(Duration::from_millis(CODEX_OUTPUT_RELOAD_CHECK_INTERVAL_MS));
    }
}

fn is_codex_output_noise_line(line: &str) -> bool {
    let lowered = line.trim().to_ascii_lowercase();
    lowered.contains("tokens used")
        || lowered.contains("token usage")
        || lowered.contains("input tokens")
        || lowered.contains("output tokens")
        || lowered.contains("total tokens")
        || lowered.contains("latency")
        || lowered.contains("elapsed")
        || lowered.starts_with("stdout")
        || lowered.starts_with("stderr")
}

fn decorate_codex_output_display_lines(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    text.lines()
        .map(|line| format!("- {}", line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn unix_timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs().to_string(),
        Err(_) => "0".to_string(),
    }
}

fn unix_timestamp_millis() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis().to_string(),
        Err(_) => "0".to_string(),
    }
}

fn send_voice_input_hotkey() -> Result<()> {
    process_runtime::send_voice_input_hotkey()
}

fn find_project_declaration_files(base_dir: &Path) -> Result<Vec<PathBuf>> {
    if !base_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let root_entries = fs::read_dir(base_dir)
        .with_context(|| format!("起動フォルダ走査に失敗: {}", base_dir.display()))?;
    for root_entry in root_entries.flatten() {
        let dir_path = root_entry.path();
        if !dir_path.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir_path) else {
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name.starts_with(PROJECT_DECLARATION_PREFIX)
                && name.ends_with(PROJECT_DECLARATION_SUFFIX)
            {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn read_project_name_from_declaration(path: &Path) -> Option<String> {
    let body = fs::read_to_string(path).ok()?;
    let first_line = body.lines().next()?.trim();
    if first_line.is_empty() {
        None
    } else {
        Some(first_line.to_string())
    }
}

fn resolve_project_debug_executable_path(declaration_path: &Path) -> Result<PathBuf> {
    let body = fs::read_to_string(declaration_path)
        .with_context(|| format!("宣言ファイル読み込みに失敗: {}", declaration_path.display()))?;
    let line_4 = body.lines().nth(3).map(str::trim).ok_or_else(|| {
        anyhow!(
            "宣言ファイルの4行目が見つかりません: {}",
            declaration_path.display()
        )
    })?;
    if line_4.is_empty() {
        return Err(anyhow!(
            "宣言ファイルの4行目にEXEパスがありません: {}",
            declaration_path.display()
        ));
    }
    let exe_path = PathBuf::from(line_4.trim_matches('"'));
    if !exe_path.is_file() {
        return Err(anyhow!(
            "debug実行ファイルが見つかりません: {}",
            exe_path.display()
        ));
    }
    Ok(exe_path)
}

fn format_system_time_hhmm(system_time: SystemTime) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

        let since_unix = system_time.duration_since(UNIX_EPOCH).ok()?;
        let unix_100ns = since_unix
            .as_secs()
            .checked_mul(10_000_000)?
            .checked_add((since_unix.subsec_nanos() / 100) as u64)?;
        let windows_epoch_offset_100ns = 11644473600_u64.checked_mul(10_000_000)?;
        let filetime_ticks = unix_100ns.checked_add(windows_epoch_offset_100ns)?;
        let file_time = FILETIME {
            dwLowDateTime: filetime_ticks as u32,
            dwHighDateTime: (filetime_ticks >> 32) as u32,
        };
        let mut utc_time = SYSTEMTIME::default();
        if unsafe { FileTimeToSystemTime(&file_time, &mut utc_time) }.is_err() {
            return None;
        }
        let mut local_time = SYSTEMTIME::default();
        if unsafe { SystemTimeToTzSpecificLocalTime(None, &utc_time, &mut local_time) }.is_err() {
            return None;
        }
        return Some(format!("{:02}:{:02}", local_time.wHour, local_time.wMinute));
    }

    #[cfg(not(windows))]
    {
        let since_unix = system_time.duration_since(UNIX_EPOCH).ok()?;
        let total_minutes = (since_unix.as_secs() / 60) % (24 * 60);
        let hour = total_minutes / 60;
        let minute = total_minutes % 60;
        Some(format!("{hour:02}:{minute:02}"))
    }
}

fn resolve_project_debug_launch_target(exe_path: &Path) -> PathBuf {
    let shortcut_candidate = exe_path.with_extension("lnk");
    if shortcut_candidate.is_file() {
        shortcut_candidate
    } else {
        exe_path.to_path_buf()
    }
}

fn launch_target_with_shell(target: &Path, working_dir: &Path) -> Result<()> {
    let status = Command::new("cmd")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg("/D")
        .arg(working_dir)
        .arg(target)
        .status()
        .with_context(|| format!("シェル起動に失敗: {}", target.display()))?;
    if !status.success() {
        return Err(anyhow!(
            "シェル起動が失敗しました status={}: {}",
            status,
            target.display()
        ));
    }
    Ok(())
}

fn is_valid_auto_start_executable_path(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

fn try_spawn_auto_start_executable(path: &Path) -> Result<()> {
    Command::new(path)
        .spawn()
        .with_context(|| format!("自動起動失敗: {}", path.display()))?;
    Ok(())
}

fn selected_repo_bridge_file_path(auto_start_exe_path: &str) -> Result<PathBuf> {
    let trimmed = auto_start_exe_path.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("自動起動設定1が未設定です"));
    }
    let exe_path = Path::new(trimmed);
    let exe_dir = exe_path.parent().ok_or_else(|| {
        anyhow!(
            "自動起動設定1の親ディレクトリを解決できません: {}",
            exe_path.display()
        )
    })?;
    Ok(exe_dir.join("runtime").join("selected_repo_path.txt"))
}

fn load_config() -> Result<AppConfig> {
    let path = config_file_path();
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let config_text = fs::read_to_string(&path)
        .with_context(|| format!("設定ファイル読み込みに失敗: {}", path.display()))?;
    let config: AppConfig = serde_json::from_str(&config_text)
        .with_context(|| format!("設定ファイル解析に失敗: {}", path.display()))?;
    Ok(config)
}

fn save_config(config: &AppConfig) -> Result<()> {
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("設定ディレクトリ作成に失敗: {}", parent.display()))?;
    }

    let body = serde_json::to_string_pretty(config).context("設定シリアライズに失敗")?;
    fs::write(&path, format!("{body}\n"))
        .with_context(|| format!("設定ファイル保存に失敗: {}", path.display()))?;
    Ok(())
}

fn config_base_dir() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("com", "gonec", "codex-shell") {
        return project_dirs.config_dir().to_path_buf();
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn config_file_path() -> PathBuf {
    config_base_dir().join("config.json")
}

fn asset_base_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir);
    }

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        candidates.push(exe_dir.to_path_buf());
        if let Some(parent) = exe_dir.parent() {
            candidates.push(parent.to_path_buf());
        }
    }

    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn ui_runtime_base_dir() -> PathBuf {
    for candidate in asset_base_candidates() {
        if candidate.join(UI_RUNTIME_RELATIVE_PATH).is_file() {
            return candidate;
        }
    }
    if let Ok(current_dir) = std::env::current_dir() {
        return current_dir;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn create_codex_output_runtime_log_path() -> Result<PathBuf> {
    let log_dir = codex_output_runtime_log_dir_path();
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("Codex出力ログディレクトリ作成に失敗: {}", log_dir.display()))?;
    let file_name = format!("codex_output_{}.log", unix_timestamp_millis());
    let log_path = log_dir.join(file_name);
    fs::write(&log_path, "")
        .with_context(|| format!("Codex出力ログ作成に失敗: {}", log_path.display()))?;
    Ok(log_path)
}

fn codex_output_runtime_log_dir_path() -> PathBuf {
    ui_runtime_base_dir().join(CODEX_OUTPUT_RUNTIME_LOG_DIR_RELATIVE_PATH)
}

fn ui_definition_file_path() -> PathBuf {
    ui_runtime_base_dir().join(UI_RUNTIME_RELATIVE_PATH)
}

fn ensure_runtime_ui_file() -> Result<PathBuf> {
    let ui_path = ui_definition_file_path();
    if ui_path.is_file() {
        return Ok(ui_path);
    }

    let runtime_base = ui_runtime_base_dir();
    let source_path = runtime_base.join(UI_INIT_RELATIVE_PATH);
    if !source_path.is_file() {
        return Err(anyhow!("UI定義が見つかりません: {}", ui_path.display()));
    }
    let body = fs::read_to_string(&source_path)
        .with_context(|| format!("UI定義移行元の読み込みに失敗: {}", source_path.display()))?;
    if let Some(parent) = ui_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("UI定義ディレクトリ作成に失敗: {}", parent.display()))?;
    }
    fs::write(&ui_path, body)
        .with_context(|| format!("UI定義移行に失敗: {}", ui_path.display()))?;

    Ok(ui_path)
}

fn load_ui_definition(path: &Path) -> Result<UiDefinition> {
    let body = fs::read_to_string(path)
        .with_context(|| format!("UI定義読み込みに失敗: {}", path.display()))?;
    let mut definition: UiDefinition = serde_json::from_str(&body)
        .with_context(|| format!("UI定義解析に失敗: {}", path.display()))?;
    definition.normalize_screens();
    Ok(definition)
}

fn save_ui_definition(path: &Path, definition: &UiDefinition) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("UI定義ディレクトリ作成に失敗: {}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(definition).context("UI定義シリアライズに失敗")?;
    fs::write(path, format!("{body}\n"))
        .with_context(|| format!("UI定義保存に失敗: {}", path.display()))?;
    Ok(())
}

fn required_asset_path(relative_path: &str) -> Result<PathBuf> {
    let candidates = asset_base_candidates()
        .into_iter()
        .map(|base| base.join(relative_path))
        .collect::<Vec<_>>();

    for path in &candidates {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
    }

    let tried = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    Err(anyhow!(
        "必須ファイルが見つかりません: {relative_path} / tried: {tried}"
    ))
}

fn apply_required_font(ctx: &egui::Context) -> Result<(PathBuf, Vec<String>)> {
    let font_path = required_asset_path(FONT_RELATIVE_PATH)?;
    let _ofl_path = required_asset_path(FONT_OFL_RELATIVE_PATH)?;
    let _source_path = required_asset_path(FONT_SOURCE_RELATIVE_PATH)?;
    let font_dir = font_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("フォントディレクトリ解決に失敗: {}", font_path.display()))?;
    let mut loaded_fonts: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in fs::read_dir(&font_dir)
        .with_context(|| format!("フォントディレクトリ読み込みに失敗: {}", font_dir.display()))?
    {
        let entry = match entry {
            Ok(value) => value,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if !matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let key = stem.replace(' ', "_");
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        loaded_fonts.push((key, bytes));
    }
    if !loaded_fonts.iter().any(|(name, _)| name == "noto_sans_jp") {
        let font_bytes = fs::read(&font_path)
            .with_context(|| format!("フォント読み込みに失敗: {}", font_path.display()))?;
        loaded_fonts.insert(0, ("noto_sans_jp".to_string(), font_bytes));
    }
    loaded_fonts.sort_by(|left, right| left.0.cmp(&right.0));
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.clear();
    fonts.families.clear();
    let mut font_names = Vec::new();
    for (font_name, font_bytes) in loaded_fonts {
        if !fonts.font_data.contains_key(&font_name) {
            fonts.font_data.insert(
                font_name.clone(),
                Arc::new(egui::FontData::from_owned(font_bytes)),
            );
            font_names.push(font_name);
        }
    }
    if font_names.is_empty() {
        return Err(anyhow!(
            "フォントが見つかりません: {}",
            font_dir.to_string_lossy()
        ));
    }
    fonts
        .families
        .insert(egui::FontFamily::Proportional, font_names.clone());
    fonts
        .families
        .insert(egui::FontFamily::Monospace, font_names.clone());
    for font_name in &font_names {
        fonts.families.insert(
            egui::FontFamily::Name(Arc::from(font_name.clone())),
            vec![font_name.clone()],
        );
    }
    ctx.set_fonts(fonts);
    Ok((font_path, font_names))
}

fn apply_visual_fix(ctx: &egui::Context) {
    let base_text = Color32::from_rgb(0, 0, 0);
    let strong_text = Color32::from_rgb(0, 0, 0);
    let weak_text = Color32::from_rgb(24, 24, 24);
    let panel_bg = Color32::WHITE;
    let button_border = Color32::from_rgb(0, 0, 0);

    ctx.set_theme(egui::Theme::Light);
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.visuals.dark_mode = false;
        style.visuals.text_alpha_from_coverage = egui::epaint::AlphaFromCoverage::Gamma(0.55);
        style.visuals.disabled_alpha = 1.0;

        style.visuals.override_text_color = Some(base_text);
        style.visuals.weak_text_color = Some(weak_text);
        style.visuals.widgets.noninteractive.fg_stroke.color = base_text;
        style.visuals.widgets.inactive.fg_stroke.color = base_text;
        style.visuals.widgets.hovered.fg_stroke.color = strong_text;
        style.visuals.widgets.active.fg_stroke.color = strong_text;
        style.visuals.widgets.open.fg_stroke.color = strong_text;

        style.visuals.panel_fill = panel_bg;
        style.visuals.window_fill = panel_bg;
        style.visuals.faint_bg_color = Color32::from_gray(250);
        style.visuals.extreme_bg_color = Color32::WHITE;
        style.visuals.widgets.noninteractive.bg_fill = panel_bg;
        style.visuals.widgets.inactive.bg_fill = Color32::from_gray(248);
        style.visuals.widgets.hovered.bg_fill = Color32::from_gray(240);
        style.visuals.widgets.active.bg_fill = Color32::from_gray(232);
        style.visuals.widgets.open.bg_fill = Color32::from_gray(232);

        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(2.0, button_border);
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(2.0, button_border);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(2.0, button_border);
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, button_border);
        style.visuals.widgets.open.bg_stroke = egui::Stroke::new(2.0, button_border);
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
        style.visuals.widgets.open.corner_radius = egui::CornerRadius::same(4);
    });
}

fn is_valid_model(model: &str) -> bool {
    MODEL_CANDIDATES.contains(&model)
}

fn is_valid_reasoning_effort(effort: &str) -> bool {
    REASONING_EFFORT_CANDIDATES.contains(&effort)
}

fn load_codex_config_value(key: &str) -> Option<String> {
    let config_path = Path::new(CODEX_CONFIG_PATH);
    let current = fs::read_to_string(config_path).ok()?;
    let pattern =
        Regex::new(format!(r#"(?m)^\s*{}\s*=\s*"(.*?)"\s*$"#, regex::escape(key)).as_str()).ok()?;
    pattern
        .captures_iter(&current)
        .filter_map(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .next()
}

fn load_model() -> String {
    let Some(model) = load_codex_config_value("model") else {
        return MODEL_CANDIDATES[0].to_string();
    };
    if is_valid_model(&model) {
        model
    } else {
        MODEL_CANDIDATES[0].to_string()
    }
}

fn load_reasoning_effort() -> String {
    let Some(effort) = load_codex_config_value("model_reasoning_effort") else {
        return "medium".to_string();
    };
    if is_valid_reasoning_effort(&effort) {
        effort
    } else {
        "medium".to_string()
    }
}

fn update_codex_config_key(key: &str, value: &str) -> Result<(), String> {
    let config_path = Path::new(CODEX_CONFIG_PATH);
    let backup_path = Path::new(CODEX_CONFIG_BACKUP_PATH);

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "設定ディレクトリ作成に失敗しました: {} ({err})",
                parent.display()
            )
        })?;
    }

    if !config_path.exists() {
        fs::write(config_path, "").map_err(|err| {
            format!(
                "設定ファイル初期化に失敗しました: {} ({err})",
                config_path.display()
            )
        })?;
    }

    fs::copy(config_path, backup_path).map_err(|err| {
        format!(
            "バックアップ作成に失敗しました: {} -> {} ({err})",
            config_path.display(),
            backup_path.display()
        )
    })?;

    let current = fs::read_to_string(config_path).map_err(|err| {
        format!(
            "設定ファイル読み込みに失敗しました: {} ({err})",
            config_path.display()
        )
    })?;

    let key_pattern =
        Regex::new(format!(r#"(?m)^\s*{}\s*=\s*".*?"\s*$"#, regex::escape(key)).as_str())
            .map_err(|err| format!("正規表現の構築に失敗しました: {err}"))?;
    let replacement = format!(r#"{key} = "{value}""#);

    let updated = if key_pattern.is_match(&current) {
        key_pattern
            .replace_all(&current, replacement.as_str())
            .into_owned()
    } else {
        let mut body = current;
        if !body.is_empty() && !body.ends_with('\n') {
            body.push('\n');
        }
        body.push_str(&replacement);
        body.push('\n');
        body
    };

    fs::write(config_path, updated).map_err(|err| {
        format!(
            "設定ファイル書き込みに失敗しました: {} ({err})",
            config_path.display()
        )
    })?;

    let verified = fs::read_to_string(config_path).map_err(|err| {
        format!(
            "更新後確認の読み込みに失敗しました: {} ({err})",
            config_path.display()
        )
    })?;
    let verify_pattern =
        Regex::new(format!(r#"(?m)^\s*{}\s*=\s*"(.*?)"\s*$"#, regex::escape(key)).as_str())
            .map_err(|err| format!("確認用正規表現の構築に失敗しました: {err}"))?;
    let reflected = verify_pattern
        .captures_iter(&verified)
        .filter_map(|caps| caps.get(1).map(|m| m.as_str()))
        .any(|v| v == value);
    if !reflected {
        return Err(format!(
            "更新後確認に失敗しました: {key} が {value} ではありません"
        ));
    }

    Ok(())
}

fn update_model(selected: &str) -> Result<(), String> {
    if !is_valid_model(selected) {
        return Err(format!("不正なモデルです: {selected}"));
    }
    update_codex_config_key("model", selected)
}

fn update_reasoning_effort(selected: &str) -> Result<(), String> {
    if !is_valid_reasoning_effort(selected) {
        return Err(format!("不正な思考深度です: {selected}"));
    }
    update_codex_config_key("model_reasoning_effort", selected)
}

pub(crate) fn run() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([FIXED_WINDOW_WIDTH, FIXED_WINDOW_HEIGHT])
            .with_min_inner_size([FIXED_WINDOW_WIDTH, FIXED_WINDOW_HEIGHT])
            .with_max_inner_size([FIXED_WINDOW_WIDTH, FIXED_WINDOW_HEIGHT])
            .with_resizable(false),
        ..Default::default()
    };

    eframe::run_native(
        "Codex Shell Wrapper",
        options,
        Box::new(|cc| {
            CodexShellApp::try_new(cc)
                .map(|app| Box::new(app) as Box<dyn eframe::App>)
                .map_err(Into::into)
        }),
    )
    .map_err(|err| anyhow!("GUI起動に失敗: {err}"))?;

    Ok(())
}
