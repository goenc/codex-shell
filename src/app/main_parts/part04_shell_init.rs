impl CodexShellApp {
    fn try_new(cc: &eframe::CreationContext<'_>) -> Result<Self> {
        let (loaded_font, ui_font_names) = apply_required_font(&cc.egui_ctx)
            .context("同梱フォント読み込みに失敗しました。assets/fonts を確認してください")?;
        apply_visual_fix(&cc.egui_ctx);

        let mut config = load_config().unwrap_or_default();
        if config.codex_command_a.trim().is_empty() {
            config.codex_command_a = if config.codex_command.trim().is_empty() {
                DEFAULT_CODEX_COMMAND.to_string()
            } else {
                config.codex_command.clone()
            };
        }
        if config.codex_command_b.trim().is_empty() {
            config.codex_command_b = config.codex_command_a.clone();
        }
        let ui_live_path = ensure_live_ui_file()?;
        let mut ui_definition = load_ui_definition(&ui_live_path)?;
        ui_definition.normalize_screens();
        ensure_project_target_label(&mut ui_definition);
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
            codex_runtime_state_b: CodexRuntimeState::Stopped,
            history: Vec::new(),
            powershell_child: None,
            build_powershell_child: None,
            active_main_pipe_name: String::new(),
            active_build_pipe_name: String::new(),
            send_tx,
            send_result_rx,
            listener_script_path,
            window_size: egui::vec2(0.0, 0.0),
            input_area_size: egui::vec2(0.0, 0.0),
            ui_font_names,
            resize_enabled: true,
            voice_input_active: false,
            pending_input_focus: false,
            build_confirm_open: false,
            ui_resize_locked_by_save: false,
            project_runtime_active: false,
            active_project_declaration_path: None,
            target_project_dir_path: None,
            project_declarations: Vec::new(),
            project_selected_index: None,
            moved_project_highlight_keys: HashSet::new(),
        };

        app.push_history(format!(
            "同梱フォントを読み込みました: {}",
            loaded_font.display()
        ));
        app.push_history(format!("UI定義を読み込みました: {}", app.ui_live_path.display()));
        app.refresh_project_declarations();
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

    fn set_codex_runtime_state(&mut self, state: CodexRuntimeState) {
        self.codex_runtime_state = state;
        if state != CodexRuntimeState::Calculating {
            self.project_runtime_active = false;
            self.active_project_declaration_path = None;
        }
    }

    fn set_codex_runtime_state_b(&mut self, state: CodexRuntimeState) {
        self.codex_runtime_state_b = state;
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

    fn stop_listener_process(&mut self) {
        if let Some(mut child) = self.powershell_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.active_main_pipe_name.clear();
        self.set_codex_runtime_state(CodexRuntimeState::Stopped);
        self.set_codex_runtime_state_b(CodexRuntimeState::Stopped);
    }

    fn stop_build_shell_process(&mut self) {
        if let Some(mut child) = self.build_powershell_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.active_build_pipe_name.clear();
        self.set_codex_runtime_state_b(CodexRuntimeState::Stopped);
    }

    fn start_listener(&mut self) {
        self.stop_listener_process();
        self.stop_build_shell_process();

        let working_dir = self.config.working_dir.trim().to_string();
        let (main_pipe_name, build_pipe_name) = self.runtime_pipe_names();

        match spawn_listener_process(&main_pipe_name, &working_dir, "相談") {
            Ok(child) => {
                let pid = child.id();
                self.powershell_child = Some(child);
                self.active_main_pipe_name = main_pipe_name.clone();
                self.update_status(format!("ConPTY待ち受け起動中 PID={pid}"));
                self.push_history(format!("ConPTY待ち受けを起動しました PID={pid}"));
                self.start_build_shell_process(&build_pipe_name, &working_dir);
            }
            Err(err) => {
                self.update_status(format!("ConPTY待ち受け起動失敗: {err}"));
                self.push_history(format!("ConPTY待ち受け起動に失敗しました: {err}"));
            }
        }
    }

    fn start_build_shell_process(&mut self, build_pipe_name: &str, working_dir: &str) {
        match spawn_listener_process(build_pipe_name, working_dir, "実装") {
            Ok(child) => {
                let pid = child.id();
                self.build_powershell_child = Some(child);
                self.active_build_pipe_name = build_pipe_name.to_string();
                self.push_history(format!(
                    "ビルド用ConPTY待ち受けを起動しました PID={pid} pipe={build_pipe_name}"
                ));
            }
            Err(err) => {
                self.update_status(format!("ビルド用ConPTY起動失敗: {err}"));
                self.push_history(format!("ビルド用ConPTY待ち受けの起動に失敗しました: {err}"));
            }
        }
    }

    fn runtime_pipe_names(&self) -> (String, String) {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let main_pipe_name = format!("{DEFAULT_PIPE_NAME}_main_{nonce}");
        let build_pipe_name = format!("{DEFAULT_PIPE_NAME}_build_{nonce}");
        (main_pipe_name, build_pipe_name)
    }

    fn main_pipe_name(&self) -> String {
        if self.active_main_pipe_name.trim().is_empty() {
            DEFAULT_PIPE_NAME.to_string()
        } else {
            self.active_main_pipe_name.clone()
        }
    }

    fn build_pipe_name(&self) -> String {
        if self.active_build_pipe_name.trim().is_empty() {
            format!("{DEFAULT_PIPE_NAME}_build")
        } else {
            self.active_build_pipe_name.clone()
        }
    }

    fn send_command(&mut self, command: String, source: &str, delay_ms: u64) {
        let pipe_name = self.main_pipe_name();
        self.send_command_to_pipe(command, source, delay_ms, pipe_name);
    }

    fn send_command_to_pipe(
        &mut self,
        command: String,
        source: &str,
        delay_ms: u64,
        pipe_name: String,
    ) {
        if command.trim().is_empty() {
            self.update_status("空コマンドは送信しません");
            return;
        }

        let request = SendRequest {
            source: source.to_string(),
            pipe_name,
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
        let build_pipe_name = self.build_pipe_name();
        self.send_command_to_pipe(command, "ビルド", BUTTON_COMMAND_DELAY_MS, build_pipe_name);
        self.pending_input_focus = true;
    }

    fn cancel_build_when_empty(&mut self) {
        self.update_status("入力欄が未入力のためビルドを送信しません");
        self.push_history("ビルド送信を中止しました: 入力欄未入力");
        self.build_confirm_open = false;
    }

}
