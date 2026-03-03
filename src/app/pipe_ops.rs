use anyhow::{Context, Result, anyhow};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use std::time::Duration;

use crate::{CONNECT_RETRY_COUNT, CONNECT_RETRY_DELAY_MS, SendRequest, SendResult};

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

pub(crate) fn spawn_send_worker(send_rx: Receiver<SendRequest>, result_tx: Sender<SendResult>) {
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
