use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
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

pub(crate) fn send_voice_input_hotkey() -> Result<()> {
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

pub(crate) fn normalize_path_for_dedup(path: &Path) -> String {
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

pub(crate) fn terminate_running_executable(path: &str) -> Result<usize> {
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
