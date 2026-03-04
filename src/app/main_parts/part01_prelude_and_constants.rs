use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use eframe::egui::{self, Color32, RichText, TextEdit};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tools::ui_edit::api as ui_tool;

use ui_tool::{
    CODEX_STATE_B, CONFIG_RESTART_LISTENER, CONFIG_SAVE, CONFIG_STARTUP_EXE_1_BROWSE,
    CONFIG_STARTUP_EXE_2_BROWSE, CONFIG_STARTUP_EXE_3_BROWSE, CONFIG_STARTUP_EXE_4_BROWSE,
    INPUT_SEND, INPUT_VOICE_TOGGLE, MODE_BUILD, MODE_CODEX_START, MODE_CODEX_START_B,
    MODE_PROJECT_DEBUG_RUN, MODE_STOP, MODE_STOP_B, NAV_BACK_MAIN, REASONING_HIGH,
    REASONING_MEDIUM, REASONING_XHIGH, UI_EDIT_TOGGLE,
    UI_SETTINGS, is_known_ui_command,
};

const DEFAULT_PIPE_NAME: &str = "codex_shell_pipe";
const DEFAULT_BUILD_COMMAND: &str = "cargo build";
const DEFAULT_CODEX_COMMAND: &str = "codex --ask-for-approval on-request --sandbox read-only";
const LISTENER_FILE_NAME: &str = "ps_pipe_listener.ps1";
#[cfg(windows)]
const CREATE_NEW_CONSOLE_FLAG: u32 = 0x0000_0010;
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

#[allow(dead_code)]
const LISTENER_SCRIPT: &str = r#"
param(
    [Parameter(Mandatory = $true)]
    [string]$PipeName,
    [Parameter(Mandatory = $true)]
    [string]$WorkingDirectory,
    [string]$WindowTitle = "相談用",
    [string]$LogFilePath = ""
)

$ErrorActionPreference = "Continue"
[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
try { chcp 65001 > $null } catch {}
try {
    if (-not [string]::IsNullOrWhiteSpace($WindowTitle)) {
        $Host.UI.RawUI.WindowTitle = $WindowTitle
    }
} catch {}

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

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern short VkKeyScanW(char ch);

    [DllImport("user32.dll")]
    private static extern uint MapVirtualKeyW(uint uCode, uint uMapType);

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
            SendCtrlC();
            return;
        }

        if (string.Equals(line, "__listener_exit__", StringComparison.Ordinal))
        {
            InjectLine("exit");
            _running = false;
            return;
        }

        InjectLine(line);
    }

    private static void InjectLine(string line)
    {
        foreach (var ch in line)
        {
            WriteTypedChar(ch);
        }
        if (ENTER_INJECT_DELAY_MS > 0)
        {
            Thread.Sleep(ENTER_INJECT_DELAY_MS);
        }
        WriteChar('\r', VK_RETURN, SCAN_RETURN, 0);
    }

    private static void WriteTypedChar(char unicodeChar)
    {
        const ushort VK_PACKET = 0xE7;
        const uint MAPVK_VK_TO_VSC = 0;
        const uint SHIFT_PRESSED = 0x0010;
        const uint LEFT_CTRL_PRESSED = 0x0008;
        const uint LEFT_ALT_PRESSED = 0x0002;

        short keyInfo = VkKeyScanW(unicodeChar);
        ushort virtualKeyCode;
        ushort virtualScanCode;
        uint controlState = 0;

        if (keyInfo == -1)
        {
            // レイアウト変換できない文字は Unicode パケットとして注入する。
            virtualKeyCode = VK_PACKET;
            virtualScanCode = 0;
        }
        else
        {
            virtualKeyCode = (ushort)(keyInfo & 0x00FF);
            byte shiftFlags = (byte)((keyInfo >> 8) & 0x00FF);
            if ((shiftFlags & 0x01) != 0)
            {
                controlState |= SHIFT_PRESSED;
            }
            if ((shiftFlags & 0x02) != 0)
            {
                controlState |= LEFT_CTRL_PRESSED;
            }
            if ((shiftFlags & 0x04) != 0)
            {
                controlState |= LEFT_ALT_PRESSED;
            }
            virtualScanCode = (ushort)MapVirtualKeyW(virtualKeyCode, MAPVK_VK_TO_VSC);
        }

        WriteChar(unicodeChar, virtualKeyCode, virtualScanCode, controlState);
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

    private static void Log(string message)
    {
        if (string.IsNullOrWhiteSpace(_logFilePath))
        {
            return;
        }
        try
        {
            File.AppendAllText(_logFilePath, message + Environment.NewLine, Utf8NoBom);
        }
        catch
        {
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
