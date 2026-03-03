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

}
