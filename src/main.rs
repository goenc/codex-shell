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
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, TerminateProcess,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_MENU, VK_RIGHT,
};

mod ui_editor;

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
const UI_LIVE_RELATIVE_PATH: &str = "runtime/ui/live/ui.json";
const UI_RELOAD_CHECK_INTERVAL_MS: u64 = 250;
const UI_MAIN_SCREEN_ID: &str = "main";
const UI_SETTINGS_SCREEN_ID: &str = "settings";
const PROJECT_DECLARATION_PREFIX: &str = "プロジェクト宣言_";
const PROJECT_DECLARATION_SUFFIX: &str = ".md";
const PROJECT_DECLARATION_NONE_LABEL: &str = "プロジェクト指定なし";
const PROJECT_TARGET_LABEL_ID: &str = "lbl_project_target";
const PROJECT_TARGET_LABEL_COMMAND: &str = "project.target_state";
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
    input_prefix: String,
    startup_exe_1: String,
    startup_exe_2: String,
    startup_exe_3: String,
    startup_exe_4: String,
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
            input_prefix: String::new(),
            startup_exe_1: String::new(),
            startup_exe_2: String::new(),
            startup_exe_3: String::new(),
            startup_exe_4: String::new(),
            show_size_overlay: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiDefinition {
    version: u32,
    assets: UiAssets,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    objects: Vec<UiObject>,
    screens: Vec<UiScreen>,
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
        if self.screen(UI_SETTINGS_SCREEN_ID).is_none() {
            self.screens.push(default_settings_screen());
        }
        self.objects.clear();
    }

    fn screen_ids(&self) -> Vec<String> {
        self.screens.iter().map(|screen| screen.id.clone()).collect()
    }

    fn screen(&self, screen_id: &str) -> Option<&UiScreen> {
        self.screens.iter().find(|screen| screen.id == screen_id)
    }

    fn screen_index(&self, screen_id: &str) -> Option<usize> {
        self.screens.iter().position(|screen| screen.id == screen_id)
    }

    fn screen_mut(&mut self, screen_id: &str) -> Option<&mut UiScreen> {
        self.screens
            .iter_mut()
            .find(|screen| screen.id == screen_id)
    }

    fn object_index_in_screen(&self, screen_id: &str, object_id: &str) -> Option<usize> {
        self.screen(screen_id)?
            .objects
            .iter()
            .position(|object| object.id == object_id)
    }

    fn screen_objects(&self, screen_id: &str) -> Option<&Vec<UiObject>> {
        Some(&self.screen(screen_id)?.objects)
    }

    fn screen_objects_mut(&mut self, screen_id: &str) -> Option<&mut Vec<UiObject>> {
        Some(&mut self.screen_mut(screen_id)?.objects)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiScreen {
    id: String,
    objects: Vec<UiObject>,
}

impl Default for UiScreen {
    fn default() -> Self {
        Self {
            id: String::new(),
            objects: Vec::new(),
        }
    }
}

fn ensure_project_target_label(definition: &mut UiDefinition) {
    let Some(objects) = definition.screen_objects_mut(UI_MAIN_SCREEN_ID) else {
        return;
    };
    if objects
        .iter()
        .any(|object| object.id == PROJECT_TARGET_LABEL_ID)
    {
        return;
    }

    let start_rect = objects
        .iter()
        .find(|object| object.id == "btn_codex_start")
        .map(|object| (object.position.y, object.size.h));
    let input_rect = objects
        .iter()
        .find(|object| object.id == "input_command")
        .map(|object| (object.position.x, object.position.y, object.size.w));

    let height = 24.0;
    let x = input_rect.map_or(24.0, |(x, _, _)| x);
    let width = input_rect.map_or(780.0, |(_, _, width)| width.max(220.0));
    let y = match (start_rect, input_rect) {
        (Some((start_y, start_h)), Some((_, input_y, _))) => {
            let start_bottom = start_y + start_h;
            let mut candidate = start_bottom + 6.0;
            if candidate + height > input_y {
                candidate = ((start_bottom + input_y - height) / 2.0).max(start_bottom);
            }
            candidate
        }
        _ => 56.0,
    };

    let mut object = create_label_object(
        PROJECT_TARGET_LABEL_ID,
        "プロジェクト無し",
        45,
        x,
        y,
        width,
        height,
        "left",
    );
    object.bind.command = PROJECT_TARGET_LABEL_COMMAND.to_string();
    objects.push(object);
}

fn default_settings_screen() -> UiScreen {
    let mut objects = vec![
        create_label_object("lbl_settings_title", "設定", 100, 24.0, 18.0, 240.0, 28.0, "left"),
        create_label_object(
            "lbl_settings_working_dir",
            "起動フォルダ",
            100,
            24.0,
            64.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_working_dir",
            "config.working_dir",
            110,
            156.0,
            64.0,
            640.0,
            24.0,
        ),
        create_label_object("lbl_settings_build", "ビルド", 100, 24.0, 96.0, 120.0, 24.0, "left"),
        create_input_object(
            "input_settings_build",
            "config.build_command",
            110,
            156.0,
            96.0,
            640.0,
            24.0,
        ),
        create_label_object("lbl_settings_codex", "Codex", 100, 24.0, 128.0, 120.0, 24.0, "left"),
        create_input_object(
            "input_settings_codex",
            "config.codex_command",
            110,
            156.0,
            128.0,
            640.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_pipe_name",
            "パイプ名",
            100,
            24.0,
            160.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_pipe_name",
            "config.pipe_name",
            110,
            156.0,
            160.0,
            640.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_input_prefix",
            "入力先頭付加",
            100,
            24.0,
            192.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_input_prefix",
            "config.input_prefix",
            110,
            156.0,
            192.0,
            640.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_1",
            "自動起動EXE1",
            100,
            24.0,
            224.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_1",
            "config.startup_exe_1",
            110,
            156.0,
            224.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_1_browse",
            "参照",
            "config.startup_exe_1.browse",
            120,
            804.0,
            224.0,
            72.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_2",
            "自動起動EXE2",
            100,
            24.0,
            252.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_2",
            "config.startup_exe_2",
            110,
            156.0,
            252.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_2_browse",
            "参照",
            "config.startup_exe_2.browse",
            120,
            804.0,
            252.0,
            72.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_3",
            "自動起動EXE3",
            100,
            24.0,
            280.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_3",
            "config.startup_exe_3",
            110,
            156.0,
            280.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_3_browse",
            "参照",
            "config.startup_exe_3.browse",
            120,
            804.0,
            280.0,
            72.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_4",
            "自動起動EXE4",
            100,
            24.0,
            308.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_4",
            "config.startup_exe_4",
            110,
            156.0,
            308.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_4_browse",
            "参照",
            "config.startup_exe_4.browse",
            120,
            804.0,
            308.0,
            72.0,
            24.0,
        ),
        create_checkbox_object(
            "chk_settings_show_size_overlay",
            "サイズ表示を表示",
            "config.show_size_overlay",
            110,
            24.0,
            336.0,
            280.0,
            28.0,
        ),
        create_button_object(
            "btn_settings_save",
            "設定保存",
            "config.save",
            120,
            24.0,
            368.0,
            120.0,
            28.0,
        ),
        create_button_object(
            "btn_settings_restart",
            "PowerShell再起動",
            "config.restart_listener",
            120,
            152.0,
            368.0,
            180.0,
            28.0,
        ),
        create_button_object(
            "btn_settings_back",
            "閉じる",
            "nav.back_main",
            120,
            340.0,
            368.0,
            120.0,
            28.0,
        ),
        create_checkbox_object(
            "chk_settings_ui_edit",
            "UI編集",
            "ui.edit.toggle",
            130,
            468.0,
            368.0,
            120.0,
            28.0,
        ),
    ];
    for object in &mut objects {
        if object.id == "chk_settings_ui_edit" {
            object.checked = false;
        }
    }
    UiScreen {
        id: UI_SETTINGS_SCREEN_ID.to_string(),
        objects,
    }
}

fn create_label_object(
    id: &str,
    text: &str,
    z_index: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    align: &str,
) -> UiObject {
    UiObject {
        id: id.to_string(),
        object_type: "label".to_string(),
        z_index,
        checked: false,
        position: UiPosition { x, y },
        size: UiSize { w, h },
        visible: true,
        enabled: true,
        bind: UiBind::default(),
        visual: UiVisual {
            text: UiText {
                value: text.to_string(),
                align: align.to_string(),
                font_size: 16.0,
                font_family: "noto_sans_jp".to_string(),
                bold: false,
                italic: false,
            },
            ..UiVisual::default()
        },
    }
}

fn create_input_object(
    id: &str,
    command: &str,
    z_index: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> UiObject {
    UiObject {
        id: id.to_string(),
        object_type: "input".to_string(),
        z_index,
        checked: false,
        position: UiPosition { x, y },
        size: UiSize { w, h },
        visible: true,
        enabled: true,
        bind: UiBind {
            command: command.to_string(),
            group: String::new(),
        },
        visual: UiVisual::default(),
    }
}

fn create_button_object(
    id: &str,
    text: &str,
    command: &str,
    z_index: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> UiObject {
    UiObject {
        id: id.to_string(),
        object_type: "button".to_string(),
        z_index,
        checked: false,
        position: UiPosition { x, y },
        size: UiSize { w, h },
        visible: true,
        enabled: true,
        bind: UiBind {
            command: command.to_string(),
            group: String::new(),
        },
        visual: UiVisual {
            text: UiText {
                value: text.to_string(),
                align: "center".to_string(),
                font_size: 16.0,
                font_family: "noto_sans_jp".to_string(),
                bold: false,
                italic: false,
            },
            ..UiVisual::default()
        },
    }
}

fn project_name_from_declaration_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let trimmed = stem.trim_start_matches(PROJECT_DECLARATION_PREFIX).trim();
    if trimmed.is_empty() {
        stem.to_string()
    } else {
        trimmed.to_string()
    }
}

fn create_checkbox_object(
    id: &str,
    text: &str,
    command: &str,
    z_index: i32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> UiObject {
    UiObject {
        id: id.to_string(),
        object_type: "checkbox".to_string(),
        z_index,
        checked: false,
        position: UiPosition { x, y },
        size: UiSize { w, h },
        visible: true,
        enabled: true,
        bind: UiBind {
            command: command.to_string(),
            group: String::new(),
        },
        visual: UiVisual {
            text: UiText {
                value: text.to_string(),
                align: "left".to_string(),
                font_size: 16.0,
                font_family: "noto_sans_jp".to_string(),
                bold: false,
                italic: false,
            },
            ..UiVisual::default()
        },
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
    z_index: i32,
    checked: bool,
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
    group: String,
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
    font_size: f32,
    font_family: String,
    bold: bool,
    italic: bool,
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

#[derive(Clone, Debug)]
struct ProjectDeclarationEntry {
    name: String,
    path: Option<PathBuf>,
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
    ui_edit_grid_visible: bool,
    ui_has_unsaved_changes: bool,
    ui_current_screen_id: String,
    ui_selected_screen_id: String,
    ui_selected_object_id: String,
    ui_selected_object_ids: Vec<String>,
    selected_reasoning_effort: String,
    input_command: String,
    status_message: String,
    codex_runtime_state: CodexRuntimeState,
    history: Vec<String>,
    powershell_child: Option<Child>,
    send_tx: Sender<SendRequest>,
    send_result_rx: Receiver<SendResult>,
    listener_script_path: PathBuf,
    window_size: egui::Vec2,
    input_area_size: egui::Vec2,
    ui_font_names: Vec<String>,
    resize_enabled: bool,
    voice_input_active: bool,
    pending_input_focus: bool,
    build_confirm_open: bool,
    project_runtime_active: bool,
    active_project_declaration_path: Option<PathBuf>,
    project_declarations: Vec<ProjectDeclarationEntry>,
    project_selected_index: Option<usize>,
    project_selector_open: bool,
}

impl CodexShellApp {
    fn try_new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
        let (loaded_font, ui_font_names) = apply_required_font(&cc.egui_ctx)
            .context("同梱フォント読み込みに失敗しました。assets/fonts を確認してください")?;
        apply_visual_fix(&cc.egui_ctx);

        let config = load_config().unwrap_or_default();
        let ui_live_path = ensure_live_ui_file()?;
        let mut ui_definition = load_ui_definition(&ui_live_path)?;
        ui_definition.normalize_screens();
        ensure_project_target_label(&mut ui_definition);
        let ui_last_modified = ui_file_modified_time(&ui_live_path).ok();
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
            ui_edit_grid_visible: true,
            ui_has_unsaved_changes: false,
            ui_current_screen_id: UI_MAIN_SCREEN_ID.to_string(),
            ui_selected_screen_id: UI_MAIN_SCREEN_ID.to_string(),
            ui_selected_object_id,
            ui_selected_object_ids,
            selected_reasoning_effort: "medium".to_string(),
            input_command: String::new(),
            status_message: "待機中".to_string(),
            codex_runtime_state: CodexRuntimeState::Stopped,
            history: Vec::new(),
            powershell_child: None,
            send_tx,
            send_result_rx,
            listener_script_path,
            window_size: egui::vec2(0.0, 0.0),
            input_area_size: egui::vec2(0.0, 0.0),
            ui_font_names,
            resize_enabled: false,
            voice_input_active: false,
            pending_input_focus: false,
            build_confirm_open: false,
            project_runtime_active: false,
            active_project_declaration_path: None,
            project_declarations: Vec::new(),
            project_selected_index: None,
            project_selector_open: false,
        };

        app.push_history(format!(
            "同梱フォントを読み込みました: {}",
            loaded_font.display()
        ));
        app.push_history(format!("UI定義を読み込みました: {}", app.ui_live_path.display()));
        app.save_config();
        app.start_listener();
        app.launch_startup_executables();
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
        let allow_resize = self.config.show_size_overlay && !self.is_main_window_resize_locked();
        if self.resize_enabled == allow_resize {
            return;
        }
        self.resize_enabled = allow_resize;

        if allow_resize {
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(10.0, 10.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(8192.0, 8192.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
        } else {
            let lock_size = if self.config.show_size_overlay
                && self.is_main_window_resize_locked()
                && self.window_size.x > 1.0
                && self.window_size.y > 1.0
            {
                self.window_size
            } else {
                egui::vec2(FIXED_WINDOW_WIDTH, FIXED_WINDOW_HEIGHT)
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(lock_size));
            ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(lock_size));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(lock_size));
        }
    }

    fn is_main_window_resize_locked(&self) -> bool {
        self.ui_edit_mode && Self::is_modal_screen(self.ui_current_screen_id.as_str())
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
        if state != CodexRuntimeState::Calculating {
            self.project_selector_open = false;
            self.project_runtime_active = false;
            self.active_project_declaration_path = None;
        }
    }

    fn runtime_background_color(&self) -> Color32 {
        if self.codex_runtime_state != CodexRuntimeState::Calculating {
            return Color32::from_rgb(224, 224, 224);
        }
        if self.project_runtime_active {
            Color32::from_rgb(225, 244, 225)
        } else {
            Color32::from_rgb(255, 248, 228)
        }
    }

    fn apply_runtime_background(&self, ctx: &egui::Context) {
        let panel_bg = self.runtime_background_color();
        ctx.style_mut_of(egui::Theme::Light, |style| {
            style.visuals.panel_fill = panel_bg;
            style.visuals.faint_bg_color = panel_bg;
            style.visuals.extreme_bg_color = panel_bg;
        });
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
        let input_body = self.input_command.trim().to_string();
        let command = if input_body.is_empty() {
            String::new()
        } else if self.config.input_prefix.trim().is_empty() {
            input_body
        } else {
            format!("{}{}", self.config.input_prefix, input_body)
        };
        self.input_command.clear();
        self.send_command(command, "入力", BUTTON_COMMAND_DELAY_MS);
        self.pending_input_focus = true;
    }

    fn send_build_command(&mut self) {
        let build_input = self.input_command.trim().to_string();
        if build_input.is_empty() {
            self.cancel_build_when_empty();
            return;
        }
        let command = if self.config.build_command.trim().is_empty() {
            build_input
        } else {
            format!("{} {}", self.config.build_command.trim_end(), build_input)
        };
        self.input_command.clear();
        self.send_command(command, "ビルド", BUTTON_COMMAND_DELAY_MS);
        self.pending_input_focus = true;
    }

    fn cancel_build_when_empty(&mut self) {
        self.update_status("入力欄が未入力のためビルドを送信しません");
        self.push_history("ビルド送信を中止しました: 入力欄未入力");
        self.build_confirm_open = false;
    }

    fn launch_startup_executables(&mut self) {
        let startup_entries = [
            ("自動起動EXE1", self.config.startup_exe_1.clone()),
            ("自動起動EXE2", self.config.startup_exe_2.clone()),
            ("自動起動EXE3", self.config.startup_exe_3.clone()),
            ("自動起動EXE4", self.config.startup_exe_4.clone()),
        ];
        let mut seen_paths = HashSet::new();
        for (label, raw) in startup_entries {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let path = trimmed.trim_matches('"');
            let normalized = normalize_path_for_dedup(Path::new(path));
            if !seen_paths.insert(normalized) {
                self.push_history(format!(
                    "{label} は同一パスが既に登録済みのため起動をスキップしました: {path}"
                ));
                continue;
            }
            match terminate_running_executable(path) {
                Ok(killed) => {
                    if killed > 0 {
                        self.push_history(format!(
                            "{label} の既存プロセスを停止しました 件数={killed}: {path}"
                        ));
                    }
                }
                Err(err) => {
                    self.update_status(format!("{label} 停止失敗: {err}"));
                    self.push_history(format!(
                        "{label} の既存プロセス停止に失敗したため自動起動を中止しました: {path} ({err})"
                    ));
                    continue;
                }
            }
            let mut command = Command::new(path);
            let working_dir = self.config.working_dir.trim();
            if !working_dir.is_empty() {
                command.current_dir(working_dir);
            }
            match command.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    self.push_history(format!("{label} を自動起動しました PID={pid}: {path}"));
                }
                Err(err) => {
                    self.update_status(format!("{label} 起動失敗: {err}"));
                    self.push_history(format!("{label} の自動起動に失敗しました: {path} ({err})"));
                }
            }
        }
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
        entries.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then(left.path.cmp(&right.path))
        });
        entries.push(ProjectDeclarationEntry {
            name: PROJECT_DECLARATION_NONE_LABEL.to_string(),
            path: None,
        });
        self.project_declarations = entries;
        self.project_selected_index = match selected_path {
            Some(path) => self
                .project_declarations
                .iter()
                .position(|entry| entry.path == path)
                .or_else(|| (!self.project_declarations.is_empty()).then_some(0)),
            None => (!self.project_declarations.is_empty()).then_some(0),
        };
    }

    fn start_selected_project_declaration(&mut self) {
        let Some(index) = self.project_selected_index else {
            self.update_status("開始対象プロジェクトがありません");
            return;
        };
        let Some(entry) = self.project_declarations.get(index).cloned() else {
            self.update_status("開始対象プロジェクトが見つかりません");
            return;
        };
        let Some(path) = entry.path else {
            self.project_selector_open = false;
            self.active_project_declaration_path = None;
            self.update_status("プロジェクト指定なしを選択しました");
            self.push_history("プロジェクト指定なしで開始しました");
            return;
        };
        if !path.is_file() {
            self.update_status(format!(
                "プロジェクト宣言ファイルが見つかりません: {}",
                path.display()
            ));
            return;
        }
        self.send_command(
            path.to_string_lossy().into_owned(),
            "プロジェクト開始",
            BUTTON_COMMAND_DELAY_MS,
        );
        self.project_selector_open = false;
        self.active_project_declaration_path = Some(path.clone());
        self.push_history(format!(
            "プロジェクト開始を送信しました: {} ({})",
            entry.name,
            path.display()
        ));
    }

    fn launch_active_project_debug_executable(&mut self) {
        let Some(declaration_path) = self.active_project_declaration_path.clone() else {
            self.update_status("開始済みプロジェクトがないためデバッグEXEを起動できません");
            self.push_history("デバッグEXE起動を中止しました: 開始済みプロジェクトなし");
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
        match terminate_running_executable(&exe_text) {
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
            .unwrap_or_else(|| exe_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf());
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

    fn browse_startup_executable(&mut self, slot: usize) {
        match select_executable_file_path() {
            Ok(Some(path)) => {
                match slot {
                    1 => self.config.startup_exe_1 = path.clone(),
                    2 => self.config.startup_exe_2 = path.clone(),
                    3 => self.config.startup_exe_3 = path.clone(),
                    4 => self.config.startup_exe_4 = path.clone(),
                    _ => return,
                }
                self.update_status(format!("自動起動EXE{slot} を設定しました"));
                self.push_history(format!("自動起動EXE{slot} を参照設定しました: {path}"));
            }
            Ok(None) => {
                self.update_status(format!("自動起動EXE{slot} の参照をキャンセルしました"));
            }
            Err(err) => {
                self.update_status(format!("自動起動EXE{slot} 参照に失敗: {err}"));
                self.push_history(format!("自動起動EXE{slot} 参照に失敗しました: {err}"));
            }
        }
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
                self.project_runtime_active = false;
                self.active_project_declaration_path = None;
                self.refresh_project_declarations();
                self.project_selector_open = true;
            } else if source == "プロジェクト開始" {
                self.update_status("プロジェクト開始を送信しました");
                self.push_history(format!("{source}: {command}"));
                        self.project_runtime_active = true;
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
                    } else if source == "プロジェクト開始" {
                        self.project_runtime_active = false;
                    }
                }
            }
        }
    }

    fn save_live_ui_definition(&mut self, summary: &str) {
        match save_ui_definition(&self.ui_live_path, &self.ui_definition) {
            Ok(()) => {
                self.ui_last_modified = ui_file_modified_time(&self.ui_live_path).ok();
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
            Ok(mut definition) => {
                definition.normalize_screens();
                ensure_project_target_label(&mut definition);
                self.ui_definition = definition;
                self.ui_last_modified = Some(modified);
                self.ui_has_unsaved_changes = false;
                if self
                    .ui_definition
                    .screen(self.ui_current_screen_id.as_str())
                    .is_none()
                {
                    self.ui_current_screen_id = UI_MAIN_SCREEN_ID.to_string();
                }
                if self
                    .ui_definition
                    .screen(self.ui_selected_screen_id.as_str())
                    .is_none()
                {
                    self.ui_selected_screen_id = self.ui_current_screen_id.clone();
                }
                if self.ui_selected_object_id.is_empty()
                    || self
                        .ui_definition
                        .object_index_in_screen(
                            self.ui_selected_screen_id.as_str(),
                            &self.ui_selected_object_id,
                        )
                        .is_none()
                {
                    self.ui_selected_object_id = self
                        .ui_definition
                        .screen_objects(self.ui_selected_screen_id.as_str())
                        .and_then(|objects| objects.first())
                        .map(|object| object.id.clone())
                        .unwrap_or_default();
                }
                let selected_screen_id = self.ui_selected_screen_id.clone();
                self.ensure_selected_objects_valid(selected_screen_id.as_str());
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
            "mode.project_debug_run" => self.active_project_declaration_path.is_some(),
            _ => true,
        }
    }

    fn runtime_checked_for_command(&self, command: &str) -> Option<bool> {
        match command.trim() {
            "ui.edit.toggle" => Some(self.ui_edit_mode),
            "reasoning.medium" => Some(self.selected_reasoning_effort == "medium"),
            "reasoning.high" => Some(self.selected_reasoning_effort == "high"),
            "reasoning.xhigh" => Some(self.selected_reasoning_effort == "xhigh"),
            "config.show_size_overlay" => Some(self.config.show_size_overlay),
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
                "ui.edit.toggle" => Some(ui_edit_mode),
                "reasoning.medium" => Some(selected_reasoning_effort == "medium"),
                "reasoning.high" => Some(selected_reasoning_effort == "high"),
                "reasoning.xhigh" => Some(selected_reasoning_effort == "xhigh"),
                "config.show_size_overlay" => Some(self.config.show_size_overlay),
                _ => None,
            };
            if let Some(desired_checked) = desired && object.checked != desired_checked {
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
            "status.message" => format!("状態: {}", self.status_message),
            "codex.state" => format!("Codex状態: {}", self.codex_runtime_state.label()),
            PROJECT_TARGET_LABEL_COMMAND => self
                .active_project_declaration_path
                .as_ref()
                .map(|path| project_name_from_declaration_path(path))
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "プロジェクト無し".to_string()),
            "ui.edit.locked_hint" => "編集モード中のため操作は無効".to_string(),
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

    fn resolve_label_color(&self, object: &UiObject) -> Color32 {
        match object.bind.command.trim() {
            PROJECT_TARGET_LABEL_COMMAND if self.active_project_declaration_path.is_some() => {
                Color32::from_rgb(255, 140, 0)
            }
            _ => Color32::BLACK,
        }
    }

    fn is_object_runtime_visible(&self, object: &UiObject) -> bool {
        if !object.visible {
            return false;
        }
        match object.bind.command.trim() {
            "ui.edit.locked_hint" => self.ui_edit_mode,
            _ => true,
        }
    }

    fn dispatch_ui_command(&mut self, command: &str) {
        match command.trim() {
            "" => {}
            "mode.codex_start" => self.send_codex_command(),
            "mode.stop" => self.request_interrupt(),
            "mode.build" => {
                if self.input_command.trim().is_empty() {
                    self.cancel_build_when_empty();
                    return;
                }
                self.build_confirm_open = true;
                self.update_status("ビルド確認待ち");
                self.push_history("ビルド確認ダイアログを表示しました");
            }
            "mode.project_debug_run" => self.launch_active_project_debug_executable(),
            "input.send" => self.send_input_command_by_button(),
            "input.voice_toggle" => self.toggle_voice_input(),
            "ui.settings" => {
                self.ui_current_screen_id = UI_SETTINGS_SCREEN_ID.to_string();
                if !self.ui_edit_mode {
                    self.ui_selected_screen_id = self.ui_current_screen_id.clone();
                }
            }
            "nav.back_main" => {
                self.ui_current_screen_id = UI_MAIN_SCREEN_ID.to_string();
                if !self.ui_edit_mode {
                    self.ui_selected_screen_id = self.ui_current_screen_id.clone();
                }
            }
            "config.save" => self.save_config(),
            "config.restart_listener" => {
                self.save_config();
                self.start_listener();
            }
            "config.startup_exe_1.browse" => self.browse_startup_executable(1),
            "config.startup_exe_2.browse" => self.browse_startup_executable(2),
            "config.startup_exe_3.browse" => self.browse_startup_executable(3),
            "config.startup_exe_4.browse" => self.browse_startup_executable(4),
            "reasoning.medium" => self.selected_reasoning_effort = "medium".to_string(),
            "reasoning.high" => self.selected_reasoning_effort = "high".to_string(),
            "reasoning.xhigh" => self.selected_reasoning_effort = "xhigh".to_string(),
            "ui.edit.toggle" => {
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
            other => {
                self.update_status(format!("未対応のUIコマンドです: {other}"));
                self.push_history(format!("未対応UIコマンド: {other}"));
            }
        }
    }

    fn render_runtime_header(&mut self, _ctx: &egui::Context) {
    }

    fn render_project_selector_window(&mut self, ctx: &egui::Context) {
        if self.ui_current_screen_id != UI_MAIN_SCREEN_ID || !self.project_selector_open {
            return;
        }

        let mut open = true;
        egui::Window::new("プロジェクト選択")
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Tooltip)
            .default_pos(egui::pos2(24.0, 56.0))
            .fixed_size(egui::vec2(360.0, 250.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(RichText::new("プロジェクト宣言").color(Color32::BLACK));
                ui.add_space(6.0);
                egui::Frame::default()
                    .stroke(egui::Stroke::new(1.0, Color32::BLACK))
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(332.0, 148.0));
                        egui::ScrollArea::vertical().max_height(148.0).show(ui, |ui| {
                            if self.project_declarations.is_empty() {
                                ui.label(
                                    RichText::new("プロジェクト宣言_*.md が見つかりません")
                                        .color(Color32::BLACK),
                                );
                            } else {
                                for (index, entry) in self.project_declarations.iter().enumerate() {
                                    if ui
                                        .selectable_label(
                                            self.project_selected_index == Some(index),
                                            entry.name.as_str(),
                                        )
                                        .clicked()
                                    {
                                        self.project_selected_index = Some(index);
                                    }
                                }
                            }
                        });
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("更新").clicked() {
                        self.refresh_project_declarations();
                    }
                    if ui
                        .add_enabled(
                            self.project_selected_index.is_some(),
                            egui::Button::new("開始"),
                        )
                        .clicked()
                    {
                        self.start_selected_project_declaration();
                    }
                });
            });
        if !open {
            self.project_selector_open = false;
        }
    }

    fn render_build_confirm_dialog(&mut self, ctx: &egui::Context) {
        if !self.build_confirm_open {
            return;
        }
        if self.input_command.trim().is_empty() {
            self.cancel_build_when_empty();
            return;
        }

        let mut open = true;
        egui::Window::new("ビルド確認")
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size(egui::vec2(360.0, 132.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(RichText::new("ビルドを実行しますか？").color(Color32::BLACK));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("はい").clicked() {
                        if self.input_command.trim().is_empty() {
                            self.cancel_build_when_empty();
                        } else {
                            self.build_confirm_open = false;
                            self.push_history("ビルド確認: はい");
                            self.send_build_command();
                        }
                    }
                    if ui.button("いいえ").clicked() {
                        self.build_confirm_open = false;
                        self.update_status("ビルドをキャンセルしました");
                        self.push_history("ビルド確認: いいえ");
                    }
                });
            });

        if !open && self.build_confirm_open {
            self.build_confirm_open = false;
            self.update_status("ビルドをキャンセルしました");
            self.push_history("ビルド確認ダイアログを閉じました");
        }
    }

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

    fn render_runtime_ui_objects(&mut self, ctx: &egui::Context) {
        let mut clicked_commands = Vec::new();
        let mut position_changed = false;
        let mut state_changed = self.sync_runtime_bound_states();
        let controls_enabled = !self.ui_edit_mode && !self.build_confirm_open;
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
                && self.ui_font_names.iter().any(|name| name == requested_family)
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
                        let main_align = match object.visual.text.align.trim() {
                            "left" => egui::Align::Min,
                            "right" => egui::Align::Max,
                            _ => egui::Align::Center,
                        };
                        let mut rich = RichText::new(text)
                            .font(text_font.clone())
                            .color(self.resolve_label_color(&object));
                        if object.visual.text.bold {
                            rich = rich.strong();
                        }
                        if object.visual.text.italic {
                            rich = rich.italics();
                        }
                        ui.allocate_ui_with_layout(
                            object_size,
                            egui::Layout::left_to_right(egui::Align::Center).with_main_align(main_align),
                            |ui| {
                                ui.add(egui::Label::new(rich).selectable(false).sense(egui::Sense::hover()));
                            },
                        );
                    }
                    "input" => {
                        let enabled = controls_enabled && object.enabled;
                        match object_command.as_str() {
                            "config.working_dir" => {
                                let response = ui.add_enabled_ui(enabled, |ui| {
                                    ui.add_sized(
                                        [object_size.x, object_size.y],
                                        TextEdit::singleline(&mut self.config.working_dir),
                                    )
                                });
                                if response.inner.changed() {
                                    state_changed = true;
                                }
                            }
                            "config.build_command" => {
                                let response = ui.add_enabled_ui(enabled, |ui| {
                                    ui.add_sized(
                                        [object_size.x, object_size.y],
                                        TextEdit::singleline(&mut self.config.build_command),
                                    )
                                });
                                if response.inner.changed() {
                                    state_changed = true;
                                }
                            }
                            "config.codex_command" => {
                                let response = ui.add_enabled_ui(enabled, |ui| {
                                    ui.add_sized(
                                        [object_size.x, object_size.y],
                                        TextEdit::singleline(&mut self.config.codex_command),
                                    )
                                });
                                if response.inner.changed() {
                                    state_changed = true;
                                }
                            }
                            "config.pipe_name" => {
                                let response = ui.add_enabled_ui(enabled, |ui| {
                                    ui.add_sized(
                                        [object_size.x, object_size.y],
                                        TextEdit::singleline(&mut self.config.pipe_name),
                                    )
                                });
                                if response.inner.changed() {
                                    state_changed = true;
                                }
                            }
                            "config.input_prefix" => {
                                let response = ui.add_enabled_ui(enabled, |ui| {
                                    ui.add_sized(
                                        [object_size.x, object_size.y],
                                        TextEdit::singleline(&mut self.config.input_prefix),
                                    )
                                });
                                if response.inner.changed() {
                                    state_changed = true;
                                }
                            }
                            "config.startup_exe_1" => {
                                let response = ui.add_enabled_ui(enabled, |ui| {
                                    ui.add_sized(
                                        [object_size.x, object_size.y],
                                        TextEdit::singleline(&mut self.config.startup_exe_1),
                                    )
                                });
                                if response.inner.changed() {
                                    state_changed = true;
                                }
                            }
                            "config.startup_exe_2" => {
                                let response = ui.add_enabled_ui(enabled, |ui| {
                                    ui.add_sized(
                                        [object_size.x, object_size.y],
                                        TextEdit::singleline(&mut self.config.startup_exe_2),
                                    )
                                });
                                if response.inner.changed() {
                                    state_changed = true;
                                }
                            }
                            "config.startup_exe_3" => {
                                let response = ui.add_enabled_ui(enabled, |ui| {
                                    ui.add_sized(
                                        [object_size.x, object_size.y],
                                        TextEdit::singleline(&mut self.config.startup_exe_3),
                                    )
                                });
                                if response.inner.changed() {
                                    state_changed = true;
                                }
                            }
                            "config.startup_exe_4" => {
                                let response = ui.add_enabled_ui(enabled, |ui| {
                                    ui.add_sized(
                                        [object_size.x, object_size.y],
                                        TextEdit::singleline(&mut self.config.startup_exe_4),
                                    )
                                });
                                if response.inner.changed() {
                                    state_changed = true;
                                }
                            }
                            _ => {
                                let input_font_id = egui::FontId::monospace(INPUT_FONT_SIZE);
                                let row_height = ui.fonts_mut(|fonts| fonts.row_height(&input_font_id));
                                let desired_rows = ((object_size.y - FIXED_INPUT_HEIGHT_PADDING)
                                    .max(row_height)
                                    / row_height)
                                    .floor()
                                    .max(1.0) as usize;
                                let frame_stroke = if object_id == "input_command" {
                                    egui::Stroke::NONE
                                } else {
                                    egui::Stroke::new(1.0, Color32::BLACK)
                                };
                                let frame_fill = if object_id == "input_command" {
                                    Color32::from_gray(242)
                                } else {
                                    Color32::WHITE
                                };
                                let input_response = egui::Frame::default()
                                    .fill(frame_fill)
                                    .stroke(frame_stroke)
                                    .inner_margin(egui::Margin::same(4))
                                    .show(ui, |ui| {
                                        let mut editor = TextEdit::multiline(&mut self.input_command)
                                            .id_source(INPUT_COMMAND_ID_SALT)
                                            .font(input_font_id)
                                            .interactive(enabled)
                                            .desired_width(f32::INFINITY)
                                            .desired_rows(desired_rows);
                                        if object_id == "input_command" {
                                            let ime_commit_this_frame = ui.input(|input| {
                                                input.events.iter().any(|event| {
                                                    matches!(
                                                        event,
                                                        egui::Event::Ime(egui::ImeEvent::Commit(_))
                                                    )
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
                                            editor = editor
                                                .frame(false)
                                                .return_key(input_return_key);
                                        }
                                        ui.add_sized(
                                            [
                                                (object_size.x - 8.0).max(1.0),
                                                (object_size.y - 8.0).max(1.0),
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
                        }
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
                    "checkbox" => {
                        let text = self.resolve_object_text(&object);
                        let enabled =
                            controls_enabled && object.enabled && self.is_bind_command_enabled(&object_command);
                        let mut checked = self
                            .runtime_checked_for_command(&object_command)
                            .unwrap_or(object.checked);
                        let mut rich = RichText::new(text).font(text_font.clone());
                        if object.visual.text.bold {
                            rich = rich.strong();
                        }
                        if object.visual.text.italic {
                            rich = rich.italics();
                        }
                        let response = ui.add_enabled_ui(enabled, |ui| {
                            ui.add_sized(
                                [object_size.x, object_size.y],
                                egui::Checkbox::new(&mut checked, rich),
                            )
                        });
                        if response.inner.changed() {
                            checkbox_changed = Some(checked);
                        }
                    }
                    "radio" | "radio_button" => {
                        let text = self.resolve_object_text(&object);
                        let enabled =
                            controls_enabled && object.enabled && self.is_bind_command_enabled(&object_command);
                        let checked = self
                            .runtime_checked_for_command(&object_command)
                            .unwrap_or(object.checked);
                        let mut rich = RichText::new(text).font(text_font.clone());
                        if object.visual.text.bold {
                            rich = rich.strong();
                        }
                        if object.visual.text.italic {
                            rich = rich.italics();
                        }
                        let response = ui.add_enabled_ui(enabled, |ui| {
                            ui.add_sized(
                                [object_size.x, object_size.y],
                                egui::RadioButton::new(checked, rich),
                            )
                        });
                        if response.inner.clicked() && !checked {
                            radio_selected = true;
                        }
                    }
                    _ => {
                        let text = self.resolve_object_text(&object);
                        let enabled =
                            controls_enabled && object.enabled && self.is_bind_command_enabled(&object_command);
                        let mut rich = RichText::new(text).font(text_font);
                        if object.visual.text.bold {
                            rich = rich.strong();
                        }
                        if object.visual.text.italic {
                            rich = rich.italics();
                        }
                        let response = ui.add_enabled_ui(enabled, |ui| {
                            ui.add_sized([object_size.x, object_size.y], egui::Button::new(rich))
                        });
                        if response.inner.clicked() {
                            clicked = true;
                        }
                    }
                });

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
                    if self.ui_selected_object_ids.is_empty() && !self.ui_selected_object_id.is_empty()
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
                    if object_command == "config.show_size_overlay" {
                        self.config.show_size_overlay = next_checked;
                    } else if !object_command.is_empty() {
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
        ctx.memory_mut(|memory| {
            let areas = memory.areas_mut();
            for layer in rendered_layers {
                areas.move_to_top(layer);
            }
        });

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
        let events = ui_editor::render_ui_editor_viewport(
            ctx,
            &mut self.ui_definition,
            &mut self.ui_selected_screen_id,
            &mut self.ui_selected_object_id,
            &mut self.ui_selected_object_ids,
            &mut self.ui_edit_grid_visible,
            &self.ui_font_names,
            self.config.show_size_overlay,
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
            self.save_live_ui_definition("UI編集内容を保存しました");
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
                [egui::pos2(x as f32, 0.0), egui::pos2(x as f32, max_y as f32)],
                egui::Stroke::new(if is_major { 1.6 } else { 1.0 }, if is_major { major_color } else { minor_color }),
            );
            x += grid_step_px;
        }

        let mut y = 0;
        while y <= max_y {
            let is_major = y % major_step_px == 0;
            painter.line_segment(
                [egui::pos2(0.0, y as f32), egui::pos2(max_x as f32, y as f32)],
                egui::Stroke::new(if is_major { 1.6 } else { 1.0 }, if is_major { major_color } else { minor_color }),
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

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_listener_process();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.apply_window_resize_policy(ctx);
        self.drain_send_results();
        self.reload_ui_definition_if_changed(ctx);
        self.window_size = ctx.content_rect().size();
        self.apply_runtime_background(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.allocate_space(egui::Vec2::ZERO);
        });
        self.render_runtime_header(ctx);
        self.render_runtime_ui_objects(ctx);
        self.render_build_confirm_dialog(ctx);
        self.render_project_selector_window(ctx);

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
    let normalized = first_line.trim_start_matches('#').trim();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn resolve_project_debug_executable_path(declaration_path: &Path) -> Result<PathBuf> {
    let project_dir = declaration_path
        .parent()
        .ok_or_else(|| anyhow!("宣言ファイルの親フォルダを取得できません: {}", declaration_path.display()))?;
    let debug_dir = project_dir.join("target").join("debug");
    if !debug_dir.is_dir() {
        return Err(anyhow!(
            "debugフォルダが見つかりません: {}",
            debug_dir.display()
        ));
    }
    let mut candidates = Vec::new();
    let entries = fs::read_dir(&debug_dir)
        .with_context(|| format!("debugフォルダ読み込みに失敗: {}", debug_dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if !path
            .extension()
            .and_then(|v| v.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            continue;
        }
        candidates.push(path);
    }
    if candidates.is_empty() {
        return Err(anyhow!(
            "debug実行ファイルが見つかりません: {}",
            debug_dir.display()
        ));
    }
    let folder_name = project_dir
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if let Some(preferred) = candidates.iter().find(|path| {
        path.file_stem()
            .and_then(|v| v.to_str())
            .is_some_and(|stem| stem.eq_ignore_ascii_case(&folder_name))
    }) {
        return Ok(preferred.clone());
    }
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }
    let list = candidates
        .iter()
        .filter_map(|path| path.file_name().and_then(|v| v.to_str()))
        .collect::<Vec<_>>()
        .join(", ");
    Err(anyhow!(
        "debug実行ファイルが複数あります。フォルダ名一致も見つかりません: {list}"
    ))
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
        if candidate.join(UI_LIVE_RELATIVE_PATH).is_file() {
            return candidate;
        }
    }
    if let Ok(current_dir) = std::env::current_dir() {
        return current_dir;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ui_live_file_path() -> PathBuf {
    ui_runtime_base_dir().join(UI_LIVE_RELATIVE_PATH)
}

fn ensure_live_ui_file() -> Result<PathBuf> {
    let live_path = ui_live_file_path();

    if !live_path.is_file() {
        return Err(anyhow!("live UI定義が見つかりません: {}", live_path.display()));
    }

    Ok(live_path)
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

fn normalize_path_for_dedup(path: &Path) -> String {
    let normalized = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    normalized
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn close_handle(handle: HANDLE) {
    if !handle.is_invalid() {
        unsafe {
            let _ = CloseHandle(handle);
        }
    }
}

fn process_image_path(pid: u32) -> Option<PathBuf> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut size = 32768u32;
    let mut buffer = vec![0u16; size as usize];
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
        .is_ok()
    };
    close_handle(process);
    if !ok || size == 0 {
        return None;
    }
    Some(PathBuf::from(String::from_utf16_lossy(
        &buffer[..size as usize],
    )))
}

fn find_process_ids_by_executable(path: &Path) -> Result<Vec<u32>> {
    let target = normalize_path_for_dedup(path);
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .context("プロセススナップショット取得に失敗")?;
    let mut process_ids = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut has_entry = unsafe { Process32FirstW(snapshot, &mut entry).is_ok() };
    while has_entry {
        let pid = entry.th32ProcessID;
        if let Some(image_path) = process_image_path(pid)
            && normalize_path_for_dedup(&image_path) == target
        {
            process_ids.push(pid);
        }
        has_entry = unsafe { Process32NextW(snapshot, &mut entry).is_ok() };
    }
    close_handle(snapshot);
    Ok(process_ids)
}

fn terminate_process_by_pid(pid: u32) -> Result<()> {
    let process = unsafe { OpenProcess(PROCESS_TERMINATE, false, pid) }
        .with_context(|| format!("プロセス終了のハンドル取得に失敗 pid={pid}"))?;
    if unsafe { TerminateProcess(process, 1).is_err() } {
        close_handle(process);
        return Err(anyhow!("プロセス終了APIが失敗しました pid={pid}"));
    }
    close_handle(process);
    Ok(())
}

fn terminate_running_executable(path: &str) -> Result<usize> {
    let process_ids = find_process_ids_by_executable(Path::new(path))
        .with_context(|| format!("実行中プロセス検索に失敗: {path}"))?;
    for pid in &process_ids {
        terminate_process_by_pid(*pid).with_context(|| format!("プロセス停止に失敗 pid={pid}"))?;
    }
    if !process_ids.is_empty() {
        thread::sleep(Duration::from_millis(200));
        let remaining = find_process_ids_by_executable(Path::new(path))
            .with_context(|| format!("停止後の実行中プロセス再確認に失敗: {path}"))?;
        if !remaining.is_empty() {
            return Err(anyhow!(
                "プロセス停止後も実行中のプロセスがあります: {}",
                remaining
                    .iter()
                    .map(|pid| pid.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    Ok(process_ids.len())
}

fn select_executable_file_path() -> Result<Option<String>> {
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.OpenFileDialog
$dialog.Filter = 'Executable Files (*.exe)|*.exe|All Files (*.*)|*.*'
$dialog.CheckFileExists = $true
$dialog.Multiselect = $false
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
    Write-Output $dialog.FileName
}
"#;
    let output = Command::new("powershell.exe")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-STA")
        .arg("-Command")
        .arg(script)
        .output()
        .context("実行ファイル参照ダイアログ起動に失敗")?;
    if !output.status.success() {
        return Err(anyhow!(
            "実行ファイル参照ダイアログ実行に失敗: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if selected.is_empty() {
        Ok(None)
    } else {
        Ok(Some(selected))
    }
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
