fn normalize_path_for_dedup(path: &Path) -> String {
    app::process_ops::normalize_path_for_dedup(path)
}

fn terminate_running_executable(path: &str) -> Result<usize> {
    app::process_ops::terminate_running_executable(path)
}

fn select_executable_file_path() -> Result<Option<String>> {
    app::process_ops::select_executable_file_path()
}

fn spawn_send_worker(send_rx: Receiver<SendRequest>, result_tx: Sender<SendResult>) {
    app::pipe_ops::spawn_send_worker(send_rx, result_tx);
}

fn maybe_run_conpty_listener_mode() -> Result<bool> {
    app::conpty_listener::maybe_run_from_args()
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
    if maybe_run_conpty_listener_mode()? {
        return Ok(());
    }

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
