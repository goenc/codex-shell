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

    fn send_command(&mut self, command: String, source: &str, delay_ms: u64) {
        if command.trim().is_empty() {
            self.update_status("空コマンドは送信しません");
            return;
        }
        let _ = delay_ms;
        self.update_status("通信機能は削除されています");
        self.push_history(format!("{source}送信は無効です: {command}"));
    }

    fn input_command_without_trailing_newlines(&self) -> String {
        self.input_command
            .trim_end_matches(['\r', '\n'])
            .to_string()
    }

    fn send_input_command_by_button(&mut self) {
        let command = self.input_command_without_trailing_newlines();
        self.input_command.clear();
        self.send_command(command, "入力", BUTTON_COMMAND_DELAY_MS);
        self.pending_input_focus = true;
    }

}
