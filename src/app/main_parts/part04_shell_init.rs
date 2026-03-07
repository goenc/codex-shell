impl CodexShellApp {
    fn try_new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
        let (loaded_font, ui_font_names) = apply_required_font(&cc.egui_ctx)
            .context("同梱フォント読み込みに失敗しました。assets/fonts を確認してください")?;
        apply_visual_fix(&cc.egui_ctx);

        let config = load_config().unwrap_or_default();
        let ui_live_path = ensure_live_ui_file()?;
        let mut ui_definition = load_ui_definition(&ui_live_path)?;
        ui_definition.normalize_screens();
        ensure_project_target_move_button(&mut ui_definition);
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
        let selected_reasoning_effort = load_reasoning_effort();

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
            selected_reasoning_effort,
            input_command: String::new(),
            status_message: "待機中".to_string(),
            codex_runtime_state: CodexRuntimeState::Stopped,
            codex_runtime_state_b: CodexRuntimeState::Stopped,
            history: Vec::new(),
            window_size: egui::vec2(0.0, 0.0),
            input_area_size: egui::vec2(0.0, 0.0),
            ui_font_names,
            resize_enabled: true,
            voice_input_active: false,
            pending_input_focus: false,
            codex_exec_in_progress: false,
            codex_exec_result_rx: None,
            ui_resize_locked_by_save: false,
            project_runtime_active: false,
            target_project_dir_path: None,
            project_declarations: Vec::new(),
            project_selected_index: None,
            moved_project_highlight_key: None,
        };

        app.push_history(format!(
            "同梱フォントを読み込みました: {}",
            loaded_font.display()
        ));
        app.push_history(format!("UI定義を読み込みました: {}", app.ui_live_path.display()));
        app.refresh_project_declarations();
        app.save_config();
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
        let allow_resize = self.ui_edit_mode && !self.ui_resize_locked_by_save;
        if self.resize_enabled == allow_resize {
            return;
        }
        self.resize_enabled = allow_resize;

        if allow_resize {
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::vec2(10.0, 10.0)));
            ctx.send_viewport_cmd(egui::ViewportCommand::MaxInnerSize(egui::vec2(8192.0, 8192.0)));
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
        let codex_a_active = self.codex_runtime_state == CodexRuntimeState::Calculating;
        let codex_b_active = self.codex_runtime_state_b == CodexRuntimeState::Calculating;
        if !codex_a_active && !codex_b_active {
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

    fn input_command_without_trailing_newlines(&self) -> String {
        self.input_command
            .trim_end_matches(['\r', '\n'])
            .to_string()
    }

    fn send_input_command_by_button(&mut self) {
        if self.codex_exec_in_progress {
            self.update_status("Codex実行中のため送信を受け付けできません");
            return;
        }
        let command = self.input_command_without_trailing_newlines();
        if command.is_empty() {
            self.update_status("入力欄が空のため送信しません");
            self.pending_input_focus = true;
            return;
        }
        self.input_command.clear();
        self.start_codex_exec(command);
        self.pending_input_focus = true;
    }

    fn start_codex_exec(&mut self, input: String) {
        let working_dir = self.config.working_dir.trim().to_string();
        let (result_tx, result_rx) = mpsc::channel::<CodexExecResult>();
        self.codex_exec_result_rx = Some(result_rx);
        self.codex_exec_in_progress = true;
        self.update_status("Codex単発実行中...");
        self.push_history(format!("Codex実行開始: {input}"));

        thread::spawn(move || {
            let mut command = Command::new("codex");
            command
                .arg("exec")
                .arg(&input)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if !working_dir.is_empty() {
                command.current_dir(working_dir);
            }

            let result = match command.output() {
                Ok(output) => CodexExecResult {
                    input,
                    status_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                    launch_error: None,
                },
                Err(err) => {
                    let message = if err.kind() == std::io::ErrorKind::NotFound {
                        "codex コマンドが見つかりません。PATH を確認してください。".to_string()
                    } else {
                        format!("codex 実行起動に失敗しました: {err}")
                    };
                    CodexExecResult {
                        input,
                        status_code: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        launch_error: Some(message),
                    }
                }
            };
            let _ = result_tx.send(result);
        });
    }

    fn drain_codex_exec_result(&mut self) {
        let Some(rx) = self.codex_exec_result_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.codex_exec_in_progress = false;
                self.codex_exec_result_rx = None;
                if let Some(error) = result.launch_error {
                    self.update_status(error.clone());
                    self.push_history(format!("Codex実行失敗: {error}"));
                    return;
                }

                let code = result.status_code.unwrap_or(-1);
                if code == 0 {
                    self.update_status("Codex単発実行が完了しました");
                    self.push_history(format!("Codex実行成功 code={code}: {}", result.input));
                } else {
                    self.update_status(format!("Codex単発実行が失敗しました code={code}"));
                    self.push_history(format!("Codex実行失敗 code={code}: {}", result.input));
                }

                if !result.stdout.is_empty() {
                    self.push_history(format!("stdout: {}", result.stdout));
                }
                if !result.stderr.is_empty() {
                    self.push_history(format!("stderr: {}", result.stderr));
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.codex_exec_in_progress = false;
                self.codex_exec_result_rx = None;
                self.update_status("Codex実行結果の受信に失敗しました");
                self.push_history("Codex実行結果受信チャネルが切断されました");
            }
        }
    }

}
