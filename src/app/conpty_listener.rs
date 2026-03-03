use anyhow::{Context, Result, anyhow};
use conpty::Process;
use interprocess::os::windows::named_pipe::{PipeListenerOptions, pipe_mode};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::RECT;
use windows::Win32::System::Console::{GetConsoleWindow, SetConsoleTitleW};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowRect, MoveWindow};
use windows::core::PCWSTR;

const MODE_FLAG: &str = "--conpty-listener";
const PIPE_NAME_FLAG: &str = "--pipe-name";
const WORKING_DIR_FLAG: &str = "--working-directory";
const WINDOW_TITLE_FLAG: &str = "--window-title";
const INTERRUPT_COMMAND: &str = "__interrupt__";
const LISTENER_EXIT_COMMAND: &str = "__listener_exit__";
const WINDOW_WIDTH_PX: i32 = 750;
const MAIN_WINDOW_TITLE: &str = "相談";
const BUILD_WINDOW_TITLE: &str = "実装";
const WINDOW_RECT_RETRY_COUNT: usize = 20;
const WINDOW_RECT_RETRY_DELAY_MS: u64 = 50;

struct ListenerArgs {
    pipe_name: String,
    working_dir: String,
    window_title: String,
}

pub(crate) fn maybe_run_from_args() -> Result<bool> {
    let Some(args) = parse_args(std::env::args())? else {
        return Ok(false);
    };
    run_listener(args)?;
    Ok(true)
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<ListenerArgs>> {
    let mut iter = args.into_iter();
    let _ = iter.next();
    let Some(mode) = iter.next() else {
        return Ok(None);
    };
    if mode != MODE_FLAG {
        return Ok(None);
    }

    let mut pipe_name: Option<String> = None;
    let mut working_dir: Option<String> = None;
    let mut window_title = "相談".to_string();

    while let Some(flag) = iter.next() {
        match flag.as_str() {
            PIPE_NAME_FLAG => {
                let value = iter
                    .next()
                    .ok_or_else(|| anyhow!("{PIPE_NAME_FLAG} の値が不足しています"))?;
                pipe_name = Some(value);
            }
            WORKING_DIR_FLAG => {
                let value = iter
                    .next()
                    .ok_or_else(|| anyhow!("{WORKING_DIR_FLAG} の値が不足しています"))?;
                working_dir = Some(value);
            }
            WINDOW_TITLE_FLAG => {
                let value = iter
                    .next()
                    .ok_or_else(|| anyhow!("{WINDOW_TITLE_FLAG} の値が不足しています"))?;
                window_title = value;
            }
            _ => return Err(anyhow!("不明な引数です: {flag}")),
        }
    }

    let pipe_name = pipe_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("pipe_name が空です"))?;
    let working_dir = working_dir
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("working_directory が空です"))?;

    Ok(Some(ListenerArgs {
        pipe_name,
        working_dir,
        window_title,
    }))
}

fn run_listener(args: ListenerArgs) -> Result<()> {
    if !Path::new(&args.working_dir).is_dir() {
        return Err(anyhow!(
            "Working directory does not exist: {}",
            args.working_dir
        ));
    }

    if !args.window_title.trim().is_empty() {
        set_console_title(args.window_title.trim());
    }
    set_console_width_px(WINDOW_WIDTH_PX);
    if args.window_title.trim() == BUILD_WINDOW_TITLE {
        shift_build_window_right_of_main();
    }

    let mut command = Command::new("pwsh.exe");
    command.arg("-NoLogo").arg("-NoProfile").arg("-NoExit");
    command.current_dir(&args.working_dir);

    let mut process = Process::spawn(command).context("ConPTY 起動に失敗")?;
    let mut pty_in = process.input().context("ConPTY入力ハンドル取得に失敗")?;
    let mut pty_out = process.output().context("ConPTY出力ハンドル取得に失敗")?;

    println!("Pipe listener started: {}", args.pipe_name);
    println!("Working directory: {}", args.working_dir);

    let fixed_window_title = args.window_title.trim().to_string();
    let output_thread = thread::spawn(move || -> Result<()> {
        let mut stdout = io::stdout().lock();
        let mut buffer = [0u8; 4096];
        loop {
            match pty_out.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    stdout
                        .write_all(&buffer[..size])
                        .context("ConPTY出力書き込みに失敗")?;
                    stdout.flush().context("ConPTY出力flushに失敗")?;
                    if !fixed_window_title.is_empty() {
                        set_console_title(&fixed_window_title);
                    }
                }
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::BrokenPipe
                            | io::ErrorKind::UnexpectedEof
                            | io::ErrorKind::ConnectionReset
                    ) =>
                {
                    break;
                }
                Err(err) => return Err(anyhow!("ConPTY出力読み込みに失敗: {err}")),
            }
        }
        Ok(())
    });

    let pipe_path = format!(r"\\.\pipe\{}", args.pipe_name);
    let listener = PipeListenerOptions::new()
        .path(Path::new(&pipe_path))
        .create_recv_only::<pipe_mode::Bytes>()
        .with_context(|| format!("Named Pipe待ち受け開始に失敗: {pipe_path}"))?;

    for connection in listener.incoming() {
        let connection = match connection {
            Ok(value) => value,
            Err(err) => {
                eprintln!("Named Pipe接続失敗: {err}");
                continue;
            }
        };
        let mut reader = BufReader::new(connection);
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .context("Named Pipe読み込みに失敗")?;
        if bytes == 0 {
            continue;
        }

        let command = line.trim_end_matches(&['\r', '\n'][..]).to_string();
        if command.is_empty() {
            continue;
        }

        if command == INTERRUPT_COMMAND {
            pty_in
                .write_all(&[0x03])
                .context("ConPTYへCtrl+C送信に失敗")?;
            pty_in.flush().context("ConPTY入力flushに失敗")?;
            continue;
        }

        if command == LISTENER_EXIT_COMMAND {
            write_line(&mut pty_in, "exit").context("ConPTYへexit送信に失敗")?;
            break;
        }

        write_line(&mut pty_in, &command).context("ConPTYへコマンド送信に失敗")?;
    }

    drop(pty_in);
    match output_thread.join() {
        Ok(result) => {
            let _ = result;
        }
        Err(_) => eprintln!("ConPTY出力スレッドの終了待機に失敗しました"),
    }
    Ok(())
}

fn write_line(writer: &mut impl Write, command: &str) -> Result<()> {
    writer
        .write_all(command.as_bytes())
        .context("コマンド書き込みに失敗")?;
    writer
        .write_all(b"\r")
        .context("改行書き込みに失敗")?;
    writer.flush().context("入力flushに失敗")?;
    Ok(())
}

fn set_console_title(title: &str) {
    let mut title_wide = title.encode_utf16().collect::<Vec<_>>();
    title_wide.push(0);
    unsafe {
        let _ = SetConsoleTitleW(PCWSTR(title_wide.as_ptr()));
    }
}

fn set_console_width_px(width_px: i32) {
    if width_px <= 0 {
        return;
    }
    unsafe {
        let hwnd = GetConsoleWindow();
        if hwnd.0.is_null() {
            return;
        }
        let mut rect = Default::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        let current_height = rect.bottom - rect.top;
        if current_height <= 0 {
            return;
        }
        let _ = MoveWindow(hwnd, rect.left, rect.top, width_px, current_height, true);
    }
}

fn shift_build_window_right_of_main() {
    let Some(main_rect) = wait_window_rect_by_title(MAIN_WINDOW_TITLE) else {
        return;
    };
    unsafe {
        let hwnd = GetConsoleWindow();
        if hwnd.0.is_null() {
            return;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        let current_width = rect.right - rect.left;
        let current_height = rect.bottom - rect.top;
        if current_width <= 0 || current_height <= 0 {
            return;
        }
        let main_width = main_rect.right - main_rect.left;
        if main_width <= 0 {
            return;
        }
        let target_x = main_rect.left + main_width;
        let _ = MoveWindow(hwnd, target_x, rect.top, current_width, current_height, true);
    }
}

fn wait_window_rect_by_title(title: &str) -> Option<RECT> {
    for _ in 0..WINDOW_RECT_RETRY_COUNT {
        if let Some(rect) = window_rect_by_title(title) {
            return Some(rect);
        }
        thread::sleep(Duration::from_millis(WINDOW_RECT_RETRY_DELAY_MS));
    }
    window_rect_by_title(title)
}

fn window_rect_by_title(title: &str) -> Option<RECT> {
    let mut title_wide = title.encode_utf16().collect::<Vec<_>>();
    title_wide.push(0);
    unsafe {
        let hwnd = FindWindowW(PCWSTR::null(), PCWSTR(title_wide.as_ptr())).ok()?;
        if hwnd.0.is_null() {
            return None;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return None;
        }
        Some(rect)
    }
}
