impl CodexShellApp {

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
                ensure_project_target_move_button(&mut definition);
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
            ui_tool::MODE_CODEX_START => self.codex_runtime_state != CodexRuntimeState::Calculating,
            ui_tool::MODE_CODEX_START_B => {
                self.codex_runtime_state_b != CodexRuntimeState::Calculating
            }
            ui_tool::MODE_PROJECT_DEBUG_RUN => self.active_project_declaration_path.is_some(),
            ui_tool::MODE_PROJECT_TARGET_MOVE => {
                self.target_project_dir_path.is_some()
                    && self.codex_runtime_state != CodexRuntimeState::Calculating
                    && self.codex_runtime_state_b != CodexRuntimeState::Calculating
            }
            _ => true,
        }
    }

    fn runtime_checked_for_command(&self, command: &str) -> Option<bool> {
        match command.trim() {
            ui_tool::UI_EDIT_TOGGLE => Some(self.ui_edit_mode),
            ui_tool::REASONING_MEDIUM => Some(self.selected_reasoning_effort == "medium"),
            ui_tool::REASONING_HIGH => Some(self.selected_reasoning_effort == "high"),
            ui_tool::REASONING_XHIGH => Some(self.selected_reasoning_effort == "xhigh"),
            ui_tool::CONFIG_SHOW_SIZE_OVERLAY => Some(self.config.show_size_overlay),
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
                ui_tool::UI_EDIT_TOGGLE => Some(ui_edit_mode),
                ui_tool::REASONING_MEDIUM => Some(selected_reasoning_effort == "medium"),
                ui_tool::REASONING_HIGH => Some(selected_reasoning_effort == "high"),
                ui_tool::REASONING_XHIGH => Some(selected_reasoning_effort == "xhigh"),
                ui_tool::CONFIG_SHOW_SIZE_OVERLAY => Some(self.config.show_size_overlay),
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
            ui_tool::STATUS_MESSAGE => format!("状態: {}", self.status_message),
            ui_tool::CODEX_STATE => format!("Codex状態: {}", self.codex_runtime_state.label()),
            CODEX_STATE_B => format!("Codex状態B: {}", self.codex_runtime_state_b.label()),
            ui_tool::PROJECT_TARGET_STATE => self
                .project_selected_index
                .and_then(|index| self.project_declarations.get(index))
                .map(|entry| entry.name.clone())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "プロジェクト未選択".to_string()),
            ui_tool::UI_EDIT_LOCKED_HINT => "編集モード中のため操作は無効".to_string(),
            ui_tool::INPUT_VOICE_TOGGLE => {
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
            ui_tool::PROJECT_TARGET_STATE if self.target_project_dir_path.is_some() => {
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
            ui_tool::UI_EDIT_LOCKED_HINT => self.ui_edit_mode,
            _ => true,
        }
    }

    fn handle_mode_codex_start(&mut self) {
        self.send_codex_command_a();
    }

    fn handle_mode_codex_start_b(&mut self) {
        self.send_codex_command_b();
    }

    fn handle_mode_stop(&mut self) {
        self.request_interrupt_a();
    }

    fn handle_mode_stop_b(&mut self) {
        self.request_interrupt_b();
    }

    fn handle_mode_build(&mut self) {
        if self.input_command.trim().is_empty() {
            self.cancel_build_when_empty();
            return;
        }
        self.build_confirm_open = true;
        self.update_status("ビルド確認待ち");
        self.push_history("ビルド確認ダイアログを表示しました");
    }

    fn handle_mode_project_debug_run(&mut self) {
        self.launch_active_project_debug_executable();
    }

    fn handle_mode_project_target_move(&mut self) {
        self.move_both_shells_to_selected_project_dir();
    }

    fn handle_input_send(&mut self) {
        self.send_input_command_by_button();
    }

    fn handle_input_voice_toggle(&mut self) {
        self.toggle_voice_input();
    }

    fn handle_ui_settings(&mut self) {
        self.ui_current_screen_id = UI_SETTINGS_SCREEN_ID.to_string();
        if !self.ui_edit_mode {
            self.ui_selected_screen_id = self.ui_current_screen_id.clone();
        }
    }

    fn handle_nav_back_main(&mut self) {
        self.ui_current_screen_id = UI_MAIN_SCREEN_ID.to_string();
        if !self.ui_edit_mode {
            self.ui_selected_screen_id = self.ui_current_screen_id.clone();
        }
        self.refresh_project_declarations();
    }

    fn handle_config_save(&mut self) {
        self.save_config();
    }

    fn handle_config_restart_listener(&mut self) {
        self.save_config();
        self.start_listener();
    }

    fn handle_browse_startup_exe(&mut self, slot: usize) {
        self.browse_startup_executable(slot);
    }

    fn handle_reasoning_effort(&mut self, effort: &str) {
        if self.selected_reasoning_effort == effort {
            return;
        }
        match update_reasoning_effort(effort) {
            Ok(()) => {
                self.selected_reasoning_effort = effort.to_string();
                self.update_status(format!("思考深度を {effort} に設定しました"));
                self.push_history(format!(
                    "config.toml を更新しました: model_reasoning_effort = \"{effort}\""
                ));
            }
            Err(err) => {
                self.update_status(format!("config.toml 更新失敗: {err}"));
                self.push_history(format!("config.toml 更新失敗: {err}"));
            }
        }
    }

    fn handle_ui_edit_toggle(&mut self) {
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
            self.ui_resize_locked_by_save = false;
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

    fn handle_unknown_ui_command(&mut self, command: &str) {
        self.update_status(format!("未対応のUIコマンドです: {command}"));
        self.push_history(format!("未対応UIコマンド: {command}"));
    }

    fn dispatch_ui_command(&mut self, command: &str) {
        let command = command.trim();
        #[cfg(debug_assertions)]
        if !command.is_empty() && !is_known_ui_command(command) {
            self.push_history(format!("未知UIコマンドを検出しました: {command}"));
        }

        match command {
            "" => {}
            MODE_CODEX_START => self.handle_mode_codex_start(),
            MODE_CODEX_START_B => self.handle_mode_codex_start_b(),
            MODE_STOP => self.handle_mode_stop(),
            MODE_STOP_B => self.handle_mode_stop_b(),
            MODE_BUILD => self.handle_mode_build(),
            MODE_PROJECT_DEBUG_RUN => self.handle_mode_project_debug_run(),
            MODE_PROJECT_TARGET_MOVE => self.handle_mode_project_target_move(),
            INPUT_SEND => self.handle_input_send(),
            INPUT_VOICE_TOGGLE => self.handle_input_voice_toggle(),
            UI_SETTINGS => self.handle_ui_settings(),
            NAV_BACK_MAIN => self.handle_nav_back_main(),
            CONFIG_SAVE => self.handle_config_save(),
            CONFIG_RESTART_LISTENER => self.handle_config_restart_listener(),
            CONFIG_STARTUP_EXE_1_BROWSE => self.handle_browse_startup_exe(1),
            CONFIG_STARTUP_EXE_2_BROWSE => self.handle_browse_startup_exe(2),
            CONFIG_STARTUP_EXE_3_BROWSE => self.handle_browse_startup_exe(3),
            CONFIG_STARTUP_EXE_4_BROWSE => self.handle_browse_startup_exe(4),
            REASONING_MEDIUM => self.handle_reasoning_effort("medium"),
            REASONING_HIGH => self.handle_reasoning_effort("high"),
            REASONING_XHIGH => self.handle_reasoning_effort("xhigh"),
            UI_EDIT_TOGGLE => self.handle_ui_edit_toggle(),
            other => self.handle_unknown_ui_command(other),
        }
    }

}
