impl CodexShellApp {

    fn launch_startup_executables(&mut self) {
        let startup_entries = [
            ("自動起動EXE1", self.config.startup_exe_1.clone()),
            ("自動起動EXE2", self.config.startup_exe_2.clone()),
            ("自動起動EXE3", self.config.startup_exe_3.clone()),
            ("自動起動EXE4", self.config.startup_exe_4.clone()),
        ];
        let mut seen_paths = HashSet::new();
        for (label, raw) in startup_entries {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let path = trimmed.trim_matches('"');
            let normalized = normalize_path_for_dedup(Path::new(path));
            if !seen_paths.insert(normalized) {
                self.push_history(format!(
                    "{label} は同一パスが既に登録済みのため起動をスキップしました: {path}"
                ));
                continue;
            }
            match terminate_running_executable(path) {
                Ok(killed) => {
                    if killed > 0 {
                        self.push_history(format!(
                            "{label} の既存プロセスを停止しました 件数={killed}: {path}"
                        ));
                    }
                }
                Err(err) => {
                    self.update_status(format!("{label} 停止失敗: {err}"));
                    self.push_history(format!(
                        "{label} の既存プロセス停止に失敗したため自動起動を中止しました: {path} ({err})"
                    ));
                    continue;
                }
            }
            let mut command = Command::new(path);
            let working_dir = self.config.working_dir.trim();
            if !working_dir.is_empty() {
                command.current_dir(working_dir);
            }
            match command.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    self.push_history(format!("{label} を自動起動しました PID={pid}: {path}"));
                }
                Err(err) => {
                    self.update_status(format!("{label} 起動失敗: {err}"));
                    self.push_history(format!("{label} の自動起動に失敗しました: {path} ({err})"));
                }
            }
        }
    }

    fn refresh_project_declarations(&mut self) {
        let base = self.config.working_dir.trim();
        if base.is_empty() {
            self.project_declarations.clear();
            self.project_selected_index = None;
            return;
        }
        let Ok(files) = find_project_declaration_files(Path::new(base)) else {
            self.project_declarations.clear();
            self.project_selected_index = None;
            return;
        };
        let selected_path = self
            .project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .map(|entry| entry.path.clone());
        let mut entries = Vec::new();
        for path in files {
            let name = read_project_name_from_declaration(&path).unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or("Unnamed Project")
                    .to_string()
            });
            entries.push(ProjectDeclarationEntry {
                name,
                path: Some(path),
            });
        }
        entries.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then(left.path.cmp(&right.path))
        });
        entries.push(ProjectDeclarationEntry {
            name: PROJECT_DECLARATION_NONE_LABEL.to_string(),
            path: None,
        });
        self.project_declarations = entries;
        self.project_selected_index = match selected_path {
            Some(path) => self
                .project_declarations
                .iter()
                .position(|entry| entry.path == path)
                .or_else(|| (!self.project_declarations.is_empty()).then_some(0)),
            None => (!self.project_declarations.is_empty()).then_some(0),
        };
    }

    fn start_selected_project_declaration(&mut self) {
        let Some(index) = self.project_selected_index else {
            self.update_status("開始対象プロジェクトがありません");
            return;
        };
        let Some(entry) = self.project_declarations.get(index).cloned() else {
            self.update_status("開始対象プロジェクトが見つかりません");
            return;
        };
        let Some(path) = entry.path else {
            self.project_selector_open = false;
            self.active_project_declaration_path = None;
            self.update_status("プロジェクト指定なしを選択しました");
            self.push_history("プロジェクト指定なしで開始しました");
            return;
        };
        if !path.is_file() {
            self.update_status(format!(
                "プロジェクト宣言ファイルが見つかりません: {}",
                path.display()
            ));
            return;
        }
        self.send_command(
            path.to_string_lossy().into_owned(),
            "プロジェクト開始",
            BUTTON_COMMAND_DELAY_MS,
        );
        self.project_selector_open = false;
        self.active_project_declaration_path = Some(path.clone());
        self.push_history(format!(
            "プロジェクト開始を送信しました: {} ({})",
            entry.name,
            path.display()
        ));
    }

    fn launch_active_project_debug_executable(&mut self) {
        let Some(declaration_path) = self.active_project_declaration_path.clone() else {
            self.update_status("開始済みプロジェクトがないためデバッグEXEを起動できません");
            self.push_history("デバッグEXE起動を中止しました: 開始済みプロジェクトなし");
            return;
        };
        let exe_path = match resolve_project_debug_executable_path(&declaration_path) {
            Ok(path) => path,
            Err(err) => {
                self.update_status(format!("デバッグEXE解決に失敗: {err}"));
                self.push_history(format!(
                    "デバッグEXE解決に失敗しました: {} ({err})",
                    declaration_path.display()
                ));
                return;
            }
        };
        let exe_text = exe_path.to_string_lossy().into_owned();
        match terminate_running_executable(&exe_text) {
            Ok(killed) => {
                if killed > 0 {
                    self.push_history(format!(
                        "デバッグEXEの既存プロセスを停止しました 件数={killed}: {}",
                        exe_path.display()
                    ));
                }
            }
            Err(err) => {
                self.update_status(format!("デバッグEXE停止失敗: {err}"));
                self.push_history(format!(
                    "デバッグEXEの既存プロセス停止に失敗したため起動を中止しました: {} ({err})",
                    exe_path.display()
                ));
                return;
            }
        }
        let project_dir = declaration_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| exe_path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf());
        let launch_target = resolve_project_debug_launch_target(&exe_path);
        match launch_target_with_shell(&launch_target, &project_dir) {
            Ok(()) => {
                self.update_status("デバッグEXEを起動しました");
                self.push_history(format!(
                    "デバッグEXEをシェル起動しました: {} (target: {})",
                    exe_path.display(),
                    launch_target.display()
                ));
            }
            Err(err) => {
                self.update_status(format!("デバッグEXE起動失敗: {err}"));
                self.push_history(format!(
                    "デバッグEXEの起動に失敗しました: {} ({err})",
                    launch_target.display()
                ));
            }
        }
    }

    fn browse_startup_executable(&mut self, slot: usize) {
        match select_executable_file_path() {
            Ok(Some(path)) => {
                match slot {
                    1 => self.config.startup_exe_1 = path.clone(),
                    2 => self.config.startup_exe_2 = path.clone(),
                    3 => self.config.startup_exe_3 = path.clone(),
                    4 => self.config.startup_exe_4 = path.clone(),
                    _ => return,
                }
                self.update_status(format!("自動起動EXE{slot} を設定しました"));
                self.push_history(format!("自動起動EXE{slot} を参照設定しました: {path}"));
            }
            Ok(None) => {
                self.update_status(format!("自動起動EXE{slot} の参照をキャンセルしました"));
            }
            Err(err) => {
                self.update_status(format!("自動起動EXE{slot} 参照に失敗: {err}"));
                self.push_history(format!("自動起動EXE{slot} 参照に失敗しました: {err}"));
            }
        }
    }

    fn toggle_voice_input(&mut self) {
        self.pending_input_focus = true;
        match send_voice_input_hotkey() {
            Ok(()) => {
                self.voice_input_active = !self.voice_input_active;
                self.update_status(format!(
                    "音声入力ホットキー送信済み: {VOICE_INPUT_HOTKEY_LABEL}"
                ));
                self.push_history(format!(
                    "音声入力ホットキー送信: {} -> {}",
                    VOICE_INPUT_HOTKEY_LABEL,
                    if self.voice_input_active {
                        "読み取り中"
                    } else {
                        "音声入力"
                    }
                ));
            }
            Err(err) => {
                self.update_status(format!("音声入力ホットキー送信失敗: {err}"));
                self.push_history(format!("音声入力ホットキー送信失敗: {err}"));
            }
        }
    }

    fn send_codex_command_a(&mut self) {
        self.send_codex_command_to("Codex", false);
    }

    fn send_codex_command_b(&mut self) {
        self.send_codex_command_to("CodexB", true);
    }

    fn send_codex_command_to(&mut self, source: &str, to_build_pipe: bool) {
        let selected = self.selected_reasoning_effort.clone();
        match update_reasoning_effort(&selected) {
            Ok(()) => {
                self.push_history(format!(
                    "config.toml を更新しました: model_reasoning_effort = \"{selected}\""
                ));
                let command = self.config.codex_command.trim().to_string();
                if to_build_pipe {
                    let pipe_name = self.build_pipe_name();
                    self.send_command_to_pipe(command, source, BUTTON_COMMAND_DELAY_MS, pipe_name);
                } else {
                    self.send_command(command, source, BUTTON_COMMAND_DELAY_MS);
                }
            }
            Err(err) => {
                self.update_status(format!("config.toml 更新失敗: {err}"));
                self.push_history(format!("config.toml 更新失敗: {err}"));
                if to_build_pipe {
                    self.set_codex_runtime_state_b(CodexRuntimeState::Stopped);
                } else {
                    self.set_codex_runtime_state(CodexRuntimeState::Stopped);
                }
            }
        }
    }

    fn request_interrupt_a(&mut self) {
        self.send_command("__interrupt__".to_string(), "停止", 0);
        self.set_codex_runtime_state(CodexRuntimeState::Stopped);
    }

    fn request_interrupt_b(&mut self) {
        let pipe_name = self.build_pipe_name();
        self.send_command_to_pipe("__interrupt__".to_string(), "停止B", 0, pipe_name);
        self.set_codex_runtime_state_b(CodexRuntimeState::Stopped);
    }

    fn drain_send_results(&mut self) {
        while let Ok(result) = self.send_result_rx.try_recv() {
            match result {
                SendResult::Sent { source, command } => {
                    if source == "停止" {
                        self.update_status("停止要求を送信しました");
                        self.push_history("停止要求を送信しました");
                        self.set_codex_runtime_state(CodexRuntimeState::Stopped);
                    } else if source == "停止B" {
                        self.update_status("停止B要求を送信しました");
                        self.push_history("停止B要求を送信しました");
                        self.set_codex_runtime_state_b(CodexRuntimeState::Stopped);
                    } else if source == "Codex" {
                        self.update_status("Codex起動コマンドを送信しました");
                        self.push_history(format!("{source}: {command}"));
                        self.set_codex_runtime_state(CodexRuntimeState::Calculating);
                        self.project_runtime_active = false;
                        self.active_project_declaration_path = None;
                        self.refresh_project_declarations();
                        self.project_selector_open = true;
                    } else if source == "CodexB" {
                        self.update_status("Codex起動Bコマンドを送信しました");
                        self.push_history(format!("{source}: {command}"));
                        self.set_codex_runtime_state_b(CodexRuntimeState::Calculating);
                    } else if source == "プロジェクト開始" {
                        self.update_status("プロジェクト開始を送信しました");
                        self.push_history(format!("{source}: {command}"));
                        self.project_runtime_active = true;
                    } else {
                        self.update_status(format!("{source}コマンド送信済み"));
                        self.push_history(format!("{source}: {command}"));
                    }
                }
                SendResult::Failed { source, error } => {
                    self.update_status(format!("送信失敗: {error}"));
                    self.push_history(format!("送信失敗 ({source}): {error}"));
                    if source == "Codex" {
                        self.set_codex_runtime_state(CodexRuntimeState::Stopped);
                    } else if source == "CodexB" || source == "停止B" {
                        self.set_codex_runtime_state_b(CodexRuntimeState::Stopped);
                    } else if source == "プロジェクト開始" {
                        self.project_runtime_active = false;
                    }
                }
            }
        }
    }

}
