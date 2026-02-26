use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use eframe::egui::{self, Color32, RichText, TextEdit};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
        }
    }
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
}

impl CodexShellApp {
    fn try_new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
        let loaded_font = apply_required_font(&cc.egui_ctx)
            .context("同梱フォント読み込みに失敗しました。assets/fonts を確認してください")?;
        apply_visual_fix(&cc.egui_ctx);

        let config = load_config().unwrap_or_default();
        let listener_script_path = listener_script_path();
        let (send_tx, send_rx) = mpsc::channel::<SendRequest>();
        let (send_result_tx, send_result_rx) = mpsc::channel::<SendResult>();
        spawn_send_worker(send_rx, send_result_tx);

        let mut app = Self {
            config,
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
        };

        app.push_history(format!(
            "同梱フォントを読み込みました: {}",
            loaded_font.display()
        ));
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

    fn send_input_command(&mut self) {
        let command = self.input_command.trim().to_string();
        self.input_command.clear();
        self.send_command(command, "入力", 0);
    }

    fn send_input_command_by_button(&mut self) {
        let command = self.input_command.trim().to_string();
        self.input_command.clear();
        self.send_command(command, "入力", BUTTON_COMMAND_DELAY_MS);
    }

    fn send_build_command(&mut self) {
        self.send_command(
            self.config.build_command.clone(),
            "ビルド",
            BUTTON_COMMAND_DELAY_MS,
        );
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

        if !open || close_by_button {
            self.show_settings_dialog = false;
        }
    }
}

impl eframe::App for CodexShellApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_send_results();

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            egui::Frame::default()
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "Codex状態: {}",
                                self.codex_runtime_state.label()
                            ))
                            .color(Color32::BLACK),
                        );
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

                        if ui.button("Codex起動").clicked() {
                            self.send_codex_command();
                        }
                        if ui.button("停止").clicked() {
                            self.request_interrupt();
                        }

                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if ui.button("設定").clicked() {
                                    self.show_settings_dialog = true;
                                }
                            },
                        );
                    });
                });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
            egui::Frame::default()
                .inner_margin(egui::Margin::same(16))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("状態: {}", self.status_message)).color(Color32::BLACK),
                    );
                    ui.add_space(8.0);

                    let button_width = 96.0;
                    let input_height = ui.available_height().max(320.0);

                    ui.horizontal(|ui| {
                        let text_width = (ui.available_width() - button_width - 8.0).max(220.0);
                        ui.add_sized(
                            [text_width, input_height],
                            TextEdit::multiline(&mut self.input_command),
                        );

                        ui.vertical(|ui| {
                            if ui
                                .add_sized([button_width, 26.0], egui::Button::new("入力送信"))
                                .clicked()
                            {
                                self.send_input_command_by_button();
                            }
                            if ui
                                .add_sized([button_width, 26.0], egui::Button::new("ビルド"))
                                .clicked()
                            {
                                self.send_build_command();
                            }
                        });
                    });
                });
            });

        self.render_settings_dialog(ctx);
    }
}

fn unix_timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs().to_string(),
        Err(_) => "0".to_string(),
    }
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
            .with_inner_size([760.0, 560.0])
            .with_min_inner_size([640.0, 500.0]),
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
