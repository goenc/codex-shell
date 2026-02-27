use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use eframe::egui::{self, Color32, RichText, TextEdit};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::Win32::Foundation::GetLastError;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_RIGHT,
};

const DEFAULT_PIPE_NAME: &str = "codex_shell_pipe";
const DEFAULT_BUILD_COMMAND: &str = "cargo build";
const DEFAULT_CODEX_COMMAND: &str = "codex --ask-for-approval on-request --sandbox read-only";
const LISTENER_FILE_NAME: &str = "ps_pipe_listener.ps1";
const CONNECT_RETRY_COUNT: usize = 20;
const CONNECT_RETRY_DELAY_MS: u64 = 120;
const MAX_HISTORY: usize = 200;
const BUTTON_COMMAND_DELAY_MS: u64 = 400;
const FONT_RELATIVE_PATH: &str = "assets/fonts/NotoSansJP-Regular.ttf";
const FONT_OFL_RELATIVE_PATH: &str = "assets/fonts/OFL.txt";
const FONT_SOURCE_RELATIVE_PATH: &str = "assets/fonts/FONT_SOURCE.txt";
const CODEX_CONFIG_PATH: &str = r"C:\Users\gonec\.codex\config.toml";
const CODEX_CONFIG_BACKUP_PATH: &str = r"C:\Users\gonec\.codex\config.toml.bak";
const UI_INIT_RELATIVE_PATH: &str = "runtime/ui/init/ui.json";
const UI_LIVE_RELATIVE_PATH: &str = "runtime/ui/live/ui.json";
const UI_RELOAD_CHECK_INTERVAL_MS: u64 = 250;
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

const LISTENER_SCRIPT: &str = r#"
param(
    [Parameter(Mandatory = $true)]
    [string]$PipeName,
    [Parameter(Mandatory = $true)]
    [string]$WorkingDirectory,
    [string]$LogFilePath = ""
)

$ErrorActionPreference = "Continue"
[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
try { chcp 65001 > $null } catch {}

if (-not ("PipeConsoleBridge" -as [type])) {
Add-Type -TypeDefinition @"
using System;
using System.ComponentModel;
using System.IO;
using System.IO.Pipes;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;

public static class PipeConsoleBridge
{
    private const int STD_INPUT_HANDLE = -10;
    private const short KEY_EVENT = 0x0001;
    private const uint ENABLE_PROCESSED_INPUT = 0x0001;
    private const int ENTER_INJECT_DELAY_MS = 350;
    private const ushort VK_RETURN = 0x0D;
    private const ushort SCAN_RETURN = 0x1C;

    private static readonly object SyncRoot = new object();
    private static Thread _thread;
    private static volatile bool _running;
    private static string _pipeName = "";
    private static string _logFilePath = "";
    private static readonly UTF8Encoding Utf8NoBom = new UTF8Encoding(false);

    [StructLayout(LayoutKind.Explicit, CharSet = CharSet.Unicode)]
    private struct INPUT_RECORD
    {
        [FieldOffset(0)]
        public short EventType;
        [FieldOffset(4)]
        public KEY_EVENT_RECORD KeyEvent;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct KEY_EVENT_RECORD
    {
        [MarshalAs(UnmanagedType.Bool)]
        public bool bKeyDown;
        public ushort wRepeatCount;
        public ushort wVirtualKeyCode;
        public ushort wVirtualScanCode;
        public char UnicodeChar;
        public uint dwControlKeyState;
    }

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GetStdHandle(int nStdHandle);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetConsoleMode(IntPtr hConsoleHandle, out uint lpMode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool SetConsoleMode(IntPtr hConsoleHandle, uint dwMode);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool WriteConsoleInputW(
        IntPtr hConsoleInput,
        INPUT_RECORD[] lpBuffer,
        uint nLength,
        out uint lpNumberOfEventsWritten
    );

    private static IntPtr InputHandle()
    {
        return GetStdHandle(STD_INPUT_HANDLE);
    }

    public static void Start(string pipeName, string logFilePath)
    {
        lock (SyncRoot)
        {
            if (_running)
            {
                return;
            }

            if (string.IsNullOrWhiteSpace(pipeName))
            {
                throw new ArgumentException("pipeName is required");
            }

            _pipeName = pipeName;
            _logFilePath = logFilePath ?? "";
            EnsureProcessedInput();
            _running = true;
            _thread = new Thread(ListenLoop);
            _thread.IsBackground = true;
            _thread.Name = "PipeConsoleBridge";
            _thread.Start();
        }
    }

    public static void Stop()
    {
        _running = false;
    }

    private static void ListenLoop()
    {
        while (_running)
        {
            try
            {
                using (var server = new NamedPipeServerStream(
                    _pipeName,
                    PipeDirection.In,
                    1,
                    PipeTransmissionMode.Byte,
                    PipeOptions.None))
                {
                    server.WaitForConnection();
                    using (var reader = new StreamReader(server, Utf8NoBom, false, 4096, true))
                    {
                        var line = reader.ReadLine();
                        if (line == null)
                        {
                            continue;
                        }
                        HandleLine(line);
                    }
                }
            }
            catch (Exception ex)
            {
                Log("LISTENER ERROR " + ex.Message);
            }
        }
    }

    private static void HandleLine(string line)
    {
        if (string.IsNullOrWhiteSpace(line))
        {
            return;
        }

        if (string.Equals(line, "__interrupt__", StringComparison.Ordinal))
        {
            LogBoundary("INTERRUPT", line);
            SendCtrlC();
            return;
        }

        if (string.Equals(line, "__listener_exit__", StringComparison.Ordinal))
        {
            LogBoundary("LISTENER EXIT", line);
            InjectLine("exit");
            _running = false;
            return;
        }

        LogBoundary("COMMAND START", line);
        InjectLine(line);
        LogBoundary("COMMAND END", line);
    }

    private static void InjectLine(string line)
    {
        foreach (var ch in line)
        {
            WriteChar(ch, 0, 0, 0);
        }
        if (ENTER_INJECT_DELAY_MS > 0)
        {
            Thread.Sleep(ENTER_INJECT_DELAY_MS);
        }
        WriteChar('\r', VK_RETURN, SCAN_RETURN, 0);
    }

    private static void SendCtrlC()
    {
        WriteChar('\u0003', 0x43, 0, 0x0008);
    }

    private static void EnsureProcessedInput()
    {
        uint mode;
        if (GetConsoleMode(InputHandle(), out mode))
        {
            SetConsoleMode(InputHandle(), mode | ENABLE_PROCESSED_INPUT);
        }
    }

    private static void WriteChar(char unicodeChar, ushort virtualKeyCode, ushort virtualScanCode, uint controlKeyState)
    {
        var down = new INPUT_RECORD
        {
            EventType = KEY_EVENT,
            KeyEvent = new KEY_EVENT_RECORD
            {
                bKeyDown = true,
                wRepeatCount = 1,
                wVirtualKeyCode = virtualKeyCode,
                wVirtualScanCode = virtualScanCode,
                UnicodeChar = unicodeChar,
                dwControlKeyState = controlKeyState
            }
        };

        var up = down;
        up.KeyEvent.bKeyDown = false;
        up.KeyEvent.UnicodeChar = '\0';

        var records = new[] { down, up };
        uint written;
        if (!WriteConsoleInputW(InputHandle(), records, (uint)records.Length, out written))
        {
            throw new Win32Exception(Marshal.GetLastWin32Error());
        }
    }

    private static void LogBoundary(string label, string line)
    {
        var stamp = DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss.fff");
        Log("==================== " + label + " " + stamp + " ====================");
        if (!string.IsNullOrWhiteSpace(line))
        {
            Log(line);
        }
    }

    private static void Log(string message)
    {
        Console.WriteLine(message);
        if (!string.IsNullOrWhiteSpace(_logFilePath))
        {
            try
            {
                File.AppendAllText(_logFilePath, message + Environment.NewLine, Utf8NoBom);
            }
            catch
            {
            }
        }
    }
}
"@
}

if (-not (Test-Path -LiteralPath $WorkingDirectory -PathType Container)) {
    throw "Working directory does not exist: $WorkingDirectory"
}
Set-Location -LiteralPath $WorkingDirectory
[PipeConsoleBridge]::Start($PipeName, $LogFilePath)
Write-Host "Pipe listener started: $PipeName"
Write-Host "Working directory: $WorkingDirectory"
"#;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct AppConfig {
    working_dir: String,
    build_command: String,
    codex_command: String,
    pipe_name: String,
    show_size_overlay: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_string()),
            build_command: DEFAULT_BUILD_COMMAND.to_string(),
            codex_command: DEFAULT_CODEX_COMMAND.to_string(),
            pipe_name: DEFAULT_PIPE_NAME.to_string(),
            show_size_overlay: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiDefinition {
    version: u32,
    assets: UiAssets,
    objects: Vec<UiObject>,
}

impl Default for UiDefinition {
    fn default() -> Self {
        Self {
            version: 1,
            assets: UiAssets::default(),
            objects: Vec::new(),
        }
    }
}

impl UiDefinition {
    fn object_index(&self, id: &str) -> Option<usize> {
        self.objects.iter().position(|object| object.id == id)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiAssets {
    base_dir: String,
    images: HashMap<String, String>,
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
struct UiObject {
    id: String,
    #[serde(rename = "type")]
    object_type: String,
    position: UiPosition,
    size: UiSize,
    visible: bool,
    enabled: bool,
    bind: UiBind,
    visual: UiVisual,
}

impl Default for UiObject {
    fn default() -> Self {
        Self {
            id: String::new(),
            object_type: "button".to_string(),
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
struct UiPosition {
    x: f32,
    y: f32,
}

impl Default for UiPosition {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiSize {
    w: f32,
    h: f32,
}

impl Default for UiSize {
    fn default() -> Self {
        Self { w: 120.0, h: 32.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
struct UiBind {
    command: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiVisual {
    background: UiBackground,
    icon: UiIcon,
    text: UiText,
    states: UiStates,
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
struct UiBackground {
    image: String,
    fit: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiIcon {
    image: String,
    anchor: String,
    offset: UiPosition,
    size: UiSize,
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
struct UiText {
    value: String,
    align: String,
}

impl Default for UiText {
    fn default() -> Self {
        Self {
            value: String::new(),
            align: "center".to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
struct UiStates {
    hover: UiStateVisual,
    pressed: UiStateVisual,
    disabled: UiStateVisual,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
struct UiStateVisual {
    background: UiBackground,
}

struct SendRequest {
    source: String,
    pipe_name: String,
    command: String,
    delay_ms: u64,
}

enum SendResult {
    Sent { source: String, command: String },
    Failed { source: String, error: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CodexRuntimeState {
    Calculating,
    Stopped,
}

impl CodexRuntimeState {
    fn label(self) -> &'static str {
        match self {
            Self::Calculating => "計算中",
            Self::Stopped => "停止中",
        }
    }
}

struct CodexShellApp {
    config: AppConfig,
    ui_definition: UiDefinition,
    ui_live_path: PathBuf,
    ui_last_modified: Option<SystemTime>,
    ui_last_reload_check: Instant,
    ui_edit_mode: bool,
    ui_selected_object_id: String,
    selected_reasoning_effort: String,
    input_command: String,
    status_message: String,
    codex_runtime_state: CodexRuntimeState,
    history: Vec<String>,
    show_settings_dialog: bool,
    powershell_child: Option<Child>,
    send_tx: Sender<SendRequest>,
    send_result_rx: Receiver<SendResult>,
    listener_script_path: PathBuf,
    window_size: egui::Vec2,
    input_area_size: egui::Vec2,
    resize_enabled: bool,
    voice_input_active: bool,
    pending_input_focus: bool,
}

impl CodexShellApp {
    fn try_new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
        let loaded_font = apply_required_font(&cc.egui_ctx)
            .context("同梱フォント読み込みに失敗しました。assets/fonts を確認してください")?;
        apply_visual_fix(&cc.egui_ctx);

        let config = load_config().unwrap_or_default();
        let ui_live_path = ensure_live_ui_file()?;
        let ui_definition = load_ui_definition(&ui_live_path)?;
        let ui_last_modified = ui_file_modified_time(&ui_live_path).ok();
        let ui_selected_object_id = ui_definition
            .objects
            .first()
            .map(|object| object.id.clone())
            .unwrap_or_default();
        let listener_script_path = listener_script_path();
        let (send_tx, send_rx) = mpsc::channel::<SendRequest>();
        let (send_result_tx, send_result_rx) = mpsc::channel::<SendResult>();
        spawn_send_worker(send_rx, send_result_tx);

        let mut app = Self {
            config,
            ui_definition,
            ui_live_path,
            ui_last_modified,
            ui_last_reload_check: Instant::now(),
            ui_edit_mode: false,
            ui_selected_object_id,
            selected_reasoning_effort: "medium".to_string(),
            input_command: String::new(),
            status_message: "待機中".to_string(),
            codex_runtime_state: CodexRuntimeState::Stopped,
            history: Vec::new(),
            show_settings_dialog: false,
            powershell_child: None,
            send_tx,
            send_result_rx,
            listener_script_path,
            window_size: egui::vec2(0.0, 0.0),
            input_area_size: egui::vec2(0.0, 0.0),
            resize_enabled: false,
            voice_input_active: false,
            pending_input_focus: false,
        };

        app.push_history(format!(
            "同梱フォントを読み込みました: {}",
            loaded_font.display()
        ));
        app.push_history(format!("UI定義を読み込みました: {}", app.ui_live_path.display()));
        app.save_config();
        app.start_listener();
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
        let allow_resize = self.config.show_size_overlay;
        if self.resize_enabled == allow_resize {
            return;
        }
        self.resize_enabled = allow_resize;

        if allow_resize {
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(10.0, 10.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(8192.0, 8192.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(
                FIXED_WINDOW_WIDTH,
                FIXED_WINDOW_HEIGHT,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(
                FIXED_WINDOW_WIDTH,
                FIXED_WINDOW_HEIGHT,
            )));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                FIXED_WINDOW_WIDTH,
                FIXED_WINDOW_HEIGHT,
            )));
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

    fn set_codex_runtime_state(&mut self, state: CodexRuntimeState) {
        self.codex_runtime_state = state;
    }

    fn stop_listener_process(&mut self) {
        if let Some(mut child) = self.powershell_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.set_codex_runtime_state(CodexRuntimeState::Stopped);
    }

    fn start_listener(&mut self) {
        self.stop_listener_process();

        if let Err(err) = write_listener_script(&self.listener_script_path) {
            self.update_status(format!("待ち受けスクリプト準備失敗: {err}"));
            self.push_history(format!("待ち受けスクリプト準備失敗: {err}"));
            return;
        }

        match spawn_listener_process(&self.config, &self.listener_script_path) {
            Ok(child) => {
                let pid = child.id();
                self.powershell_child = Some(child);
                self.update_status(format!("PowerShell待ち受け起動中 PID={pid}"));
                self.push_history(format!("PowerShell待ち受けを起動しました PID={pid}"));
            }
            Err(err) => {
                self.update_status(format!("PowerShell起動失敗: {err}"));
                self.push_history(format!("PowerShell起動に失敗しました: {err}"));
            }
        }
    }

    fn send_command(&mut self, command: String, source: &str, delay_ms: u64) {
        if command.trim().is_empty() {
            self.update_status("空コマンドは送信しません");
            return;
        }

        let request = SendRequest {
            source: source.to_string(),
            pipe_name: self.config.pipe_name.trim().to_string(),
            command,
            delay_ms,
        };
        if self.send_tx.send(request).is_ok() {
            if delay_ms == 0 {
                self.update_status(format!("{source}送信要求を受け付けました"));
            } else {
                self.update_status(format!(
                    "{source}送信要求を受け付けました ({delay_ms}ms遅延)"
                ));
            }
        } else {
            self.update_status("送信処理スレッドが停止しています");
            self.push_history(format!("送信失敗 ({source}): 送信処理スレッド停止"));
        }
    }

    fn send_input_command_by_button(&mut self) {
        let command = self.input_command.trim().to_string();
        self.input_command.clear();
        self.send_command(command, "入力", BUTTON_COMMAND_DELAY_MS);
        self.pending_input_focus = true;
    }

    fn send_build_command(&mut self) {
        self.send_command(
            self.config.build_command.clone(),
            "ビルド",
            BUTTON_COMMAND_DELAY_MS,
        );
    }

    fn toggle_voice_input(&mut self) {
        self.pending_input_focus = true;
        match send_voice_input_hotkey() {
            Ok(()) => {
                self.voice_input_active = !self.voice_input_active;
                self.update_status(format!(
                    "音声入力ホットキー送信済み: {VOICE_INPUT_HOTKEY_LABEL}"
                ));
                self.push_history(format!(
                    "音声入力ホットキー送信: {} -> {}",
                    VOICE_INPUT_HOTKEY_LABEL,
                    if self.voice_input_active {
                        "読み取り中"
                    } else {
                        "音声入力"
                    }
                ));
            }
            Err(err) => {
                self.update_status(format!("音声入力ホットキー送信失敗: {err}"));
                self.push_history(format!("音声入力ホットキー送信失敗: {err}"));
            }
        }
    }

    fn send_codex_command(&mut self) {
        let selected = self.selected_reasoning_effort.clone();
        match update_reasoning_effort(&selected) {
            Ok(()) => {
                self.push_history(format!(
                    "config.toml を更新しました: model_reasoning_effort = \"{selected}\""
                ));
                let command = self.config.codex_command.trim().to_string();
                self.send_command(command, "Codex", BUTTON_COMMAND_DELAY_MS);
            }
            Err(err) => {
                self.update_status(format!("config.toml 更新失敗: {err}"));
                self.push_history(format!("config.toml 更新失敗: {err}"));
                self.set_codex_runtime_state(CodexRuntimeState::Stopped);
            }
        }
    }

    fn request_interrupt(&mut self) {
        self.send_command("__interrupt__".to_string(), "停止", 0);
        self.set_codex_runtime_state(CodexRuntimeState::Stopped);
    }

    fn drain_send_results(&mut self) {
        while let Ok(result) = self.send_result_rx.try_recv() {
            match result {
                SendResult::Sent { source, command } => {
                    if source == "停止" {
                        self.update_status("停止要求を送信しました");
                        self.push_history("停止要求を送信しました");
                        self.set_codex_runtime_state(CodexRuntimeState::Stopped);
                    } else if source == "Codex" {
                        self.update_status("Codex起動コマンドを送信しました");
                        self.push_history(format!("{source}: {command}"));
                        self.set_codex_runtime_state(CodexRuntimeState::Calculating);
                    } else {
                        self.update_status(format!("{source}コマンド送信済み"));
                        self.push_history(format!("{source}: {command}"));
                    }
                }
                SendResult::Failed { source, error } => {
                    self.update_status(format!("送信失敗: {error}"));
                    self.push_history(format!("送信失敗 ({source}): {error}"));
                    if source == "Codex" {
                        self.set_codex_runtime_state(CodexRuntimeState::Stopped);
                    }
                }
            }
        }
    }

    fn save_live_ui_definition(&mut self, summary: &str) {
        match save_ui_definition(&self.ui_live_path, &self.ui_definition) {
            Ok(()) => {
                self.ui_last_modified = ui_file_modified_time(&self.ui_live_path).ok();
                self.push_history(summary);
            }
            Err(err) => {
                self.update_status(format!("UI定義保存失敗: {err}"));
                self.push_history(format!("UI定義保存に失敗しました: {err}"));
            }
        }
    }

    fn reload_ui_definition_if_changed(&mut self, ctx: &egui::Context) {
        if self.ui_last_reload_check.elapsed() < Duration::from_millis(UI_RELOAD_CHECK_INTERVAL_MS) {
            return;
        }
        self.ui_last_reload_check = Instant::now();

        if !self.ui_live_path.exists() {
            match ensure_live_ui_file() {
                Ok(path) => {
                    self.ui_live_path = path;
                }
                Err(err) => {
                    self.update_status(format!("UI定義復元失敗: {err}"));
                    return;
                }
            }
        }

        let modified = match ui_file_modified_time(&self.ui_live_path) {
            Ok(modified) => modified,
            Err(err) => {
                self.update_status(format!("UI定義時刻取得失敗: {err}"));
                return;
            }
        };

        if self.ui_last_modified == Some(modified) {
            return;
        }

        match load_ui_definition(&self.ui_live_path) {
            Ok(definition) => {
                self.ui_definition = definition;
                self.ui_last_modified = Some(modified);
                if self.ui_selected_object_id.is_empty()
                    || self
                        .ui_definition
                        .object_index(&self.ui_selected_object_id)
                        .is_none()
                {
                    self.ui_selected_object_id = self
                        .ui_definition
                        .objects
                        .first()
                        .map(|object| object.id.clone())
                        .unwrap_or_default();
                }
                self.push_history("UI定義を再読み込みしました");
                ctx.request_repaint();
            }
            Err(err) => {
                self.update_status(format!("UI定義再読み込み失敗: {err}"));
            }
        }
    }

    fn is_bind_command_enabled(&self, command: &str) -> bool {
        match command.trim() {
            "mode.codex_start" => self.codex_runtime_state != CodexRuntimeState::Calculating,
            _ => true,
        }
    }

    fn resolve_object_text(&self, object: &UiObject) -> String {
        match object.bind.command.trim() {
            "status.message" => format!("状態: {}", self.status_message),
            "codex.state" => format!("Codex状態: {}", self.codex_runtime_state.label()),
            "input.voice_toggle" => {
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

    fn dispatch_ui_command(&mut self, command: &str) {
        match command.trim() {
            "" => {}
            "mode.codex_start" => self.send_codex_command(),
            "mode.stop" => self.request_interrupt(),
            "mode.build" => self.send_build_command(),
            "input.send" => self.send_input_command_by_button(),
            "input.voice_toggle" => self.toggle_voice_input(),
            "ui.settings" => self.show_settings_dialog = true,
            "ui.edit.toggle" => {
                self.ui_edit_mode = !self.ui_edit_mode;
                self.update_status(if self.ui_edit_mode {
                    "UI編集モードを有効化しました"
                } else {
                    "UI編集モードを無効化しました"
                });
            }
            other => {
                self.update_status(format!("未対応のUIコマンドです: {other}"));
                self.push_history(format!("未対応UIコマンド: {other}"));
            }
        }
    }

    fn render_runtime_header(&mut self, ctx: &egui::Context) {
        let mut edit_toggle_changed = false;
        egui::Area::new(egui::Id::new("runtime_header"))
            .fixed_pos(egui::pos2(UI_BASE_OUTER_MARGIN, 8.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(Color32::from_white_alpha(245))
                    .stroke(egui::Stroke::new(1.0, Color32::from_gray(150)))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "Codex状態: {}",
                                    self.codex_runtime_state.label()
                                ))
                                .color(Color32::BLACK),
                            );
                            let controls_enabled = !self.ui_edit_mode;
                            ui.add_enabled_ui(controls_enabled, |ui| {
                                ui.label(RichText::new("思考深度").color(Color32::BLACK));
                                ui.radio_value(
                                    &mut self.selected_reasoning_effort,
                                    "medium".to_string(),
                                    "medium",
                                );
                                ui.radio_value(
                                    &mut self.selected_reasoning_effort,
                                    "high".to_string(),
                                    "high",
                                );
                                ui.radio_value(
                                    &mut self.selected_reasoning_effort,
                                    "xhigh".to_string(),
                                    "xhigh",
                                );
                                if ui.checkbox(&mut self.ui_edit_mode, "UI編集").changed() {
                                    edit_toggle_changed = true;
                                }
                            });
                            if self.ui_edit_mode {
                                ui.label(
                                    RichText::new("編集モード中のため操作は無効")
                                        .color(Color32::from_rgb(128, 0, 0)),
                                );
                            }
                        });
                    });
            });

        if edit_toggle_changed {
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
            if self.ui_selected_object_id.is_empty() {
                self.ui_selected_object_id = self
                    .ui_definition
                    .objects
                    .first()
                    .map(|object| object.id.clone())
                    .unwrap_or_default();
            }
        }
    }

    fn render_runtime_ui_objects(&mut self, ctx: &egui::Context) {
        let mut clicked_commands = Vec::new();
        let mut position_changed = false;
        let controls_enabled = !self.ui_edit_mode;

        for index in 0..self.ui_definition.objects.len() {
            let object = self.ui_definition.objects[index].clone();
            if !object.visible {
                continue;
            }

            let object_type = object.object_type.trim().to_string();
            let object_id = object.id.clone();
            let object_command = object.bind.command.trim().to_string();
            let object_size = egui::vec2(object.size.w.max(12.0), object.size.h.max(12.0));
            let mut clicked = false;

            let area_response = egui::Area::new(egui::Id::new(("ui_object", object_id.clone())))
                .order(if object_type == "panel" {
                    egui::Order::Background
                } else {
                    egui::Order::Foreground
                })
                .movable(self.ui_edit_mode)
                .fixed_pos(egui::pos2(object.position.x, object.position.y))
                .show(ctx, |ui| match object_type.as_str() {
                    "panel" => {
                        let fill = if object.visual.background.image.trim().is_empty() {
                            Color32::from_gray(250)
                        } else {
                            Color32::from_gray(242)
                        };
                        egui::Frame::default()
                            .fill(fill)
                            .stroke(egui::Stroke::new(1.0, Color32::BLACK))
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| {
                                ui.set_min_size(object_size);
                            });
                    }
                    "label" => {
                        let text = self.resolve_object_text(&object);
                        ui.add_sized(
                            [object_size.x, object_size.y],
                            egui::Label::new(RichText::new(text).color(Color32::BLACK)),
                        );
                    }
                    "input" => {
                        let input_font_id = egui::FontId::monospace(INPUT_FONT_SIZE);
                        let row_height = ui.fonts_mut(|fonts| fonts.row_height(&input_font_id));
                        let desired_rows = ((object_size.y - FIXED_INPUT_HEIGHT_PADDING)
                            .max(row_height)
                            / row_height)
                            .floor()
                            .max(1.0) as usize;
                        let input_response = egui::Frame::default()
                            .fill(Color32::WHITE)
                            .stroke(egui::Stroke::new(1.0, Color32::BLACK))
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| {
                                ui.add_sized(
                                    [(object_size.x - 8.0).max(1.0), (object_size.y - 8.0).max(1.0)],
                                    TextEdit::multiline(&mut self.input_command)
                                        .id_source(INPUT_COMMAND_ID_SALT)
                                        .font(input_font_id)
                                        .interactive(controls_enabled)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(desired_rows),
                                )
                            });
                        if controls_enabled && self.pending_input_focus {
                            input_response.inner.request_focus();
                            self.pending_input_focus = false;
                        }
                        self.input_area_size = input_response.response.rect.size();
                    }
                    "image" => {
                        let image_key = object.visual.background.image.trim();
                        let text = if image_key.is_empty() {
                            "image".to_string()
                        } else {
                            format!("image: {image_key}")
                        };
                        egui::Frame::default()
                            .fill(Color32::from_gray(245))
                            .stroke(egui::Stroke::new(1.0, Color32::BLACK))
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| {
                                ui.set_min_size(object_size);
                                ui.label(RichText::new(text).color(Color32::BLACK));
                            });
                    }
                    _ => {
                        let text = self.resolve_object_text(&object);
                        let enabled =
                            controls_enabled && object.enabled && self.is_bind_command_enabled(&object_command);
                        let response = ui.add_enabled_ui(enabled, |ui| {
                            ui.add_sized([object_size.x, object_size.y], egui::Button::new(text))
                        });
                        if response.inner.clicked() {
                            clicked = true;
                        }
                    }
                });

            if self.ui_edit_mode && self.ui_selected_object_id == object_id {
                let highlight_rect = area_response.response.rect.expand(2.0);
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new(("ui_selected_highlight", object_id.clone())),
                ));
                painter.rect(
                    highlight_rect,
                    egui::CornerRadius::same(2),
                    Color32::from_rgba_unmultiplied(255, 0, 0, 26),
                    egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 0, 0, 180)),
                    egui::StrokeKind::Outside,
                );
            }

            if self.ui_edit_mode {
                let moved_to = area_response.response.rect.min;
                let target = &mut self.ui_definition.objects[index];
                if (target.position.x - moved_to.x).abs() >= 0.5
                    || (target.position.y - moved_to.y).abs() >= 0.5
                {
                    target.position.x = moved_to.x.round();
                    target.position.y = moved_to.y.round();
                    position_changed = true;
                }
            }

            if clicked && !object_command.is_empty() {
                clicked_commands.push(object_command);
            }
        }

        if position_changed && !ctx.input(|input| input.pointer.primary_down()) {
            self.save_live_ui_definition("UIオブジェクト位置を更新しました");
        }

        if controls_enabled {
            for command in clicked_commands {
                self.dispatch_ui_command(&command);
            }
        }
    }

    fn render_ui_editor_contents(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("オブジェクトをドラッグすると位置を変更できます").color(Color32::BLACK),
        );
        ui.add_space(6.0);

        if self.ui_definition.objects.is_empty() {
            ui.label(RichText::new("objects が空です").color(Color32::BLACK));
            return;
        }
        if self.ui_selected_object_id.is_empty() {
            self.ui_selected_object_id = self.ui_definition.objects[0].id.clone();
        }

        egui::ComboBox::from_label("対象オブジェクト")
            .selected_text(self.ui_selected_object_id.clone())
            .show_ui(ui, |ui| {
                for object in &self.ui_definition.objects {
                    ui.selectable_value(
                        &mut self.ui_selected_object_id,
                        object.id.clone(),
                        object.id.as_str(),
                    );
                }
            });

        if let Some(index) = self.ui_definition.object_index(&self.ui_selected_object_id) {
            let mut changed = false;
            let object = &mut self.ui_definition.objects[index];

            ui.label(RichText::new(format!("type: {}", object.object_type)).color(Color32::BLACK));
            changed |= ui.checkbox(&mut object.visible, "visible").changed();
            changed |= ui.checkbox(&mut object.enabled, "enabled").changed();

            ui.horizontal(|ui| {
                ui.label("x");
                changed |= ui
                    .add(egui::DragValue::new(&mut object.position.x).speed(1.0))
                    .changed();
                ui.label("y");
                changed |= ui
                    .add(egui::DragValue::new(&mut object.position.y).speed(1.0))
                    .changed();
            });

            ui.horizontal(|ui| {
                ui.label("w");
                changed |= ui
                    .add(egui::DragValue::new(&mut object.size.w).speed(1.0))
                    .changed();
                ui.label("h");
                changed |= ui
                    .add(egui::DragValue::new(&mut object.size.h).speed(1.0))
                    .changed();
            });

            ui.label(RichText::new("bind.command").color(Color32::BLACK));
            changed |= ui.text_edit_singleline(&mut object.bind.command).changed();

            ui.label(RichText::new("visual.text.value").color(Color32::BLACK));
            changed |= ui.text_edit_singleline(&mut object.visual.text.value).changed();

            ui.label(RichText::new("visual.background.image").color(Color32::BLACK));
            changed |= ui
                .text_edit_singleline(&mut object.visual.background.image)
                .changed();

            ui.label(RichText::new("visual.background.fit").color(Color32::BLACK));
            changed |= ui
                .text_edit_singleline(&mut object.visual.background.fit)
                .changed();

            if changed {
                self.save_live_ui_definition("UI編集で定義を更新しました");
            }
        } else {
            ui.label(RichText::new("選択オブジェクトが見つかりません").color(Color32::BLACK));
        }
    }

    fn render_ui_editor(&mut self, ctx: &egui::Context) {
        if !self.ui_edit_mode {
            return;
        }

        let viewport_id = egui::ViewportId::from_hash_of("ui_editor_viewport");
        let builder = egui::ViewportBuilder::default()
            .with_title("UI編集")
            .with_inner_size([360.0, 520.0])
            .with_min_inner_size([320.0, 420.0])
            .with_resizable(true)
            .with_close_button(true);

        ctx.show_viewport_immediate(viewport_id, builder, |editor_ctx, viewport_class| {
            if editor_ctx.input(|input| input.viewport().close_requested()) {
                self.ui_edit_mode = false;
                self.update_status("UI編集モードを無効化しました");
                self.push_history("UI編集ウィンドウを閉じました");
                return;
            }

            if viewport_class == egui::ViewportClass::Embedded {
                egui::Window::new("UI編集")
                    .default_width(340.0)
                    .resizable(true)
                    .show(editor_ctx, |ui| {
                        self.render_ui_editor_contents(ui);
                    });
            } else {
                egui::CentralPanel::default().show(editor_ctx, |ui| {
                    self.render_ui_editor_contents(ui);
                });
            }
            editor_ctx.request_repaint_after(Duration::from_millis(UI_RELOAD_CHECK_INTERVAL_MS));
        });
    }

    fn render_settings_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_settings_dialog {
            return;
        }

        let mut open = true;
        let mut close_by_button = false;
        egui::Window::new("設定")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                let controls_enabled = !self.ui_edit_mode;
                ui.add_enabled_ui(controls_enabled, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("設定").strong().color(Color32::BLACK));
                    });
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        ui.label(RichText::new("起動フォルダ").color(Color32::BLACK));
                        ui.add_sized(
                            [380.0, 24.0],
                            TextEdit::singleline(&mut self.config.working_dir),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("ビルド").color(Color32::BLACK));
                        ui.add_sized(
                            [380.0, 24.0],
                            TextEdit::singleline(&mut self.config.build_command),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Codex").color(Color32::BLACK));
                        ui.add_sized(
                            [380.0, 24.0],
                            TextEdit::singleline(&mut self.config.codex_command),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("パイプ名").color(Color32::BLACK));
                        ui.add_sized(
                            [380.0, 24.0],
                            TextEdit::singleline(&mut self.config.pipe_name),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.checkbox(
                            &mut self.config.show_size_overlay,
                            RichText::new("サイズ表示を表示").color(Color32::BLACK),
                        );
                    });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("設定保存").clicked() {
                            self.save_config();
                        }
                        if ui.button("PowerShell再起動").clicked() {
                            self.save_config();
                            self.start_listener();
                        }
                        if ui.button("閉じる").clicked() {
                            close_by_button = true;
                        }
                    });
                });
                if self.ui_edit_mode {
                    ui.label(
                        RichText::new("編集モード中のため設定操作は無効です")
                            .color(Color32::from_rgb(128, 0, 0)),
                    );
                }
            });

        if !open || close_by_button {
            self.show_settings_dialog = false;
        }
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

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_listener_process();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_window_resize_policy(ctx);
        self.drain_send_results();
        self.reload_ui_definition_if_changed(ctx);
        self.window_size = ctx.content_rect().size();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.allocate_space(egui::Vec2::ZERO);
        });
        self.render_runtime_header(ctx);
        self.render_runtime_ui_objects(ctx);

        if self.config.show_size_overlay {
            egui::Area::new(egui::Id::new("size_overlay"))
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
                .interactable(false)
                .show(ctx, |ui| {
                    egui::Frame::default()
                        .fill(Color32::from_white_alpha(232))
                        .stroke(egui::Stroke::new(1.0, Color32::from_gray(140)))
                        .inner_margin(egui::Margin::same(8))
                        .show(ui, |ui| {
                            let win_x = self.window_size.x.max(0.0).round() as i32;
                            let win_y = self.window_size.y.max(0.0).round() as i32;
                            let input_x = self.input_area_size.x.max(0.0).round() as i32;
                            let input_y = self.input_area_size.y.max(0.0).round() as i32;
                            ui.label(
                                RichText::new(format!("ウィンサイズ x={win_x} y={win_y}"))
                                    .color(Color32::BLACK),
                            );
                            ui.label(
                                RichText::new(format!("入力サイズ x={input_x} y={input_y}"))
                                    .color(Color32::BLACK),
                            );
                        });
                });
        }

        self.render_settings_dialog(ctx);
        self.render_ui_editor(ctx);
        ctx.request_repaint_after(Duration::from_millis(UI_RELOAD_CHECK_INTERVAL_MS));
    }
}

fn unix_timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs().to_string(),
        Err(_) => "0".to_string(),
    }
}

fn keyboard_input(vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_voice_input_hotkey() -> Result<()> {
    let inputs = [
        keyboard_input(VK_CONTROL, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_MENU, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_RIGHT, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(VK_RIGHT, KEYEVENTF_KEYUP),
        keyboard_input(VK_MENU, KEYEVENTF_KEYUP),
        keyboard_input(VK_CONTROL, KEYEVENTF_KEYUP),
    ];

    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent != inputs.len() as u32 {
        let err = unsafe { GetLastError() };
        return Err(anyhow!(
            "SendInput失敗 sent={sent}/{} last_error={}",
            inputs.len(),
            err.0
        ));
    }
    Ok(())
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

fn listener_script_path() -> PathBuf {
    config_base_dir().join(LISTENER_FILE_NAME)
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
        if candidate.join(UI_INIT_RELATIVE_PATH).is_file() {
            return candidate;
        }
    }
    if let Ok(current_dir) = std::env::current_dir() {
        return current_dir;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ui_init_file_path() -> PathBuf {
    ui_runtime_base_dir().join(UI_INIT_RELATIVE_PATH)
}

fn ui_live_file_path() -> PathBuf {
    ui_runtime_base_dir().join(UI_LIVE_RELATIVE_PATH)
}

fn ensure_live_ui_file() -> Result<PathBuf> {
    let init_path = ui_init_file_path();
    let live_path = ui_live_file_path();

    if !init_path.is_file() {
        return Err(anyhow!(
            "初期UI定義が見つかりません: {}",
            init_path.display()
        ));
    }

    if !live_path.exists() {
        if let Some(parent) = live_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("live UIディレクトリ作成に失敗: {}", parent.display()))?;
        }
        fs::copy(&init_path, &live_path).with_context(|| {
            format!(
                "初期UI定義コピーに失敗: {} -> {}",
                init_path.display(),
                live_path.display()
            )
        })?;
    }

    Ok(live_path)
}

fn load_ui_definition(path: &Path) -> Result<UiDefinition> {
    let body = fs::read_to_string(path)
        .with_context(|| format!("UI定義読み込みに失敗: {}", path.display()))?;
    let definition: UiDefinition = serde_json::from_str(&body)
        .with_context(|| format!("UI定義解析に失敗: {}", path.display()))?;
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

fn ui_file_modified_time(path: &Path) -> Result<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .with_context(|| format!("UI定義更新時刻取得に失敗: {}", path.display()))
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

fn apply_required_font(ctx: &egui::Context) -> Result<PathBuf> {
    let font_path = required_asset_path(FONT_RELATIVE_PATH)?;
    let _ofl_path = required_asset_path(FONT_OFL_RELATIVE_PATH)?;
    let _source_path = required_asset_path(FONT_SOURCE_RELATIVE_PATH)?;

    let font_bytes = fs::read(&font_path)
        .with_context(|| format!("フォント読み込みに失敗: {}", font_path.display()))?;
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.clear();
    fonts.families.clear();
    fonts.font_data.insert(
        "noto_sans_jp".to_string(),
        Arc::new(egui::FontData::from_owned(font_bytes)),
    );
    fonts.families.insert(
        egui::FontFamily::Proportional,
        vec!["noto_sans_jp".to_string()],
    );
    fonts.families.insert(
        egui::FontFamily::Monospace,
        vec!["noto_sans_jp".to_string()],
    );
    ctx.set_fonts(fonts);
    Ok(font_path)
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

fn write_listener_script(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "待ち受けスクリプトディレクトリ作成に失敗: {}",
                parent.display()
            )
        })?;
    }

    let mut script_bytes = Vec::with_capacity(3 + LISTENER_SCRIPT.len());
    script_bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    script_bytes.extend_from_slice(LISTENER_SCRIPT.as_bytes());

    fs::write(path, script_bytes)
        .with_context(|| format!("待ち受けスクリプト保存に失敗: {}", path.display()))?;
    Ok(())
}

fn spawn_listener_process(config: &AppConfig, script_path: &Path) -> Result<Child> {
    let child = Command::new("powershell.exe")
        .arg("-NoExit")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(script_path)
        .arg("-PipeName")
        .arg(config.pipe_name.trim())
        .arg("-WorkingDirectory")
        .arg(config.working_dir.trim())
        .spawn()
        .with_context(|| {
            format!(
                "PowerShell起動に失敗: script={}",
                script_path.to_string_lossy()
            )
        })?;
    Ok(child)
}

fn send_named_pipe_line(pipe_name: &str, command: &str) -> Result<()> {
    let pipe_path = format!(r"\\.\pipe\{}", pipe_name.trim());
    let mut last_error: Option<io::Error> = None;

    for _ in 0..CONNECT_RETRY_COUNT {
        match OpenOptions::new().write(true).open(&pipe_path) {
            Ok(mut pipe) => {
                pipe.write_all(command.as_bytes())
                    .context("パイプ書き込みに失敗")?;
                pipe.write_all(b"\n").context("改行送信に失敗")?;
                pipe.flush().context("パイプflushに失敗")?;
                return Ok(());
            }
            Err(err) => {
                last_error = Some(err);
                thread::sleep(Duration::from_millis(CONNECT_RETRY_DELAY_MS));
            }
        }
    }

    Err(anyhow!(
        "パイプ接続に失敗: {} ({})",
        pipe_path,
        last_error
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown error".to_string())
    ))
}

fn spawn_send_worker(send_rx: Receiver<SendRequest>, result_tx: Sender<SendResult>) {
    thread::spawn(move || {
        while let Ok(request) = send_rx.recv() {
            let delay_ms = request.delay_ms;
            let pipe_name = request.pipe_name;
            let source = request.source;
            let command = request.command;
            if delay_ms > 0 {
                thread::sleep(Duration::from_millis(delay_ms));
            }
            match send_named_pipe_line(&pipe_name, &command) {
                Ok(()) => {
                    if result_tx
                        .send(SendResult::Sent { source, command })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(err) => {
                    if result_tx
                        .send(SendResult::Failed {
                            source,
                            error: err.to_string(),
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    });
}

fn update_reasoning_effort(selected: &str) -> Result<(), String> {
    if !matches!(selected, "medium" | "high" | "xhigh") {
        return Err(format!("不正な思考深度です: {selected}"));
    }

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

    let key_pattern = Regex::new(r#"model_reasoning_effort\s*=\s*".*?""#)
        .map_err(|err| format!("正規表現の構築に失敗しました: {err}"))?;
    let replacement = format!(r#"model_reasoning_effort = "{selected}""#);

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
    let verify_pattern = Regex::new(r#"model_reasoning_effort\s*=\s*"(.*?)""#)
        .map_err(|err| format!("確認用正規表現の構築に失敗しました: {err}"))?;
    let reflected = verify_pattern
        .captures_iter(&verified)
        .filter_map(|caps| caps.get(1).map(|m| m.as_str()))
        .any(|value| value == selected);
    if !reflected {
        return Err(format!(
            "更新後確認に失敗しました: model_reasoning_effort が {selected} ではありません"
        ));
    }

    Ok(())
}

fn main() -> Result<()> {
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
