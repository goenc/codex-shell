impl CodexShellApp {
    fn project_entry_highlight_key(entry: &ProjectDeclarationEntry) -> String {
        if let Some(path) = entry.path.as_ref() {
            normalize_path_for_dedup(path)
        } else {
            entry.name.trim().to_ascii_lowercase()
        }
    }

    fn selected_project_highlight_key(&self) -> Option<String> {
        self.project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .map(Self::project_entry_highlight_key)
    }

    fn is_selected_project_highlighted(&self) -> bool {
        self.selected_project_highlight_key()
            .is_some_and(|key| self.moved_project_highlight_key.as_deref() == Some(key.as_str()))
    }

    fn is_project_launch_ready(&self) -> bool {
        self.is_selected_project_highlighted()
    }

    fn sync_selected_project_target_dir(&mut self) {
        self.target_project_dir_path = self
            .project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .and_then(|entry| entry.path.as_ref())
            .and_then(|path| path.parent().map(Path::to_path_buf));
    }

    fn selected_project_declaration_path(&self) -> Option<PathBuf> {
        self.project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .and_then(|entry| entry.path.as_ref())
            .cloned()
    }

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
            match count_running_executable(path) {
                Ok(running) => {
                    if running > 0 {
                        self.push_history(format!(
                            "{label} は既に起動中のため自動起動をスキップしました 件数={running}: {path}"
                        ));
                        continue;
                    }
                }
                Err(err) => {
                    self.update_status(format!("{label} 起動確認失敗: {err}"));
                    self.push_history(format!(
                        "{label} の起動確認に失敗したため自動起動を中止しました: {path} ({err})"
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
        self.project_declarations = entries;
        self.project_selected_index = match selected_path {
            Some(path) => self
                .project_declarations
                .iter()
                .position(|entry| entry.path == path)
                .or_else(|| (!self.project_declarations.is_empty()).then_some(0)),
            None => (!self.project_declarations.is_empty()).then_some(0),
        };
        self.sync_selected_project_target_dir();
    }

    fn launch_active_project_debug_executable(&mut self) {
        if !self.is_project_launch_ready() {
            self.update_status("緑ハイライトのプロジェクトが未選択のためデバッグEXEを起動できません");
            self.push_history("デバッグEXE起動を中止しました: 緑ハイライト未選択");
            return;
        }
        let Some(declaration_path) = self.selected_project_declaration_path() else {
            self.update_status("プロジェクト未選択のためデバッグEXEを起動できません");
            self.push_history("デバッグEXE起動を中止しました: プロジェクト未選択");
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

    fn active_project_debug_modified_hhmm(&self) -> Option<String> {
        if !self.is_project_launch_ready() {
            return None;
        }
        let declaration_path = self.selected_project_declaration_path()?;
        let exe_path = resolve_project_debug_executable_path(&declaration_path).ok()?;
        let modified = fs::metadata(exe_path).ok()?.modified().ok()?;
        format_system_time_hhmm(modified)
    }

    fn move_both_shells_to_selected_project_dir(&mut self) {
        let Some(target_dir) = self.target_project_dir_path.clone() else {
            self.update_status("移動対象のプロジェクトフォルダが未選択です");
            self.push_history("プロジェクトフォルダ移動を中止しました: 未選択");
            return;
        };
        self.moved_project_highlight_key = self.selected_project_highlight_key();
        self.update_status("通信機能削除のため作業フォルダ移動コマンドは実行しません");
        self.push_history(format!(
            "作業フォルダ移動コマンドは無効です: {}",
            target_dir.display()
        ));
        let startup_executables = vec![
            self.config.startup_exe_1.clone(),
            self.config.startup_exe_2.clone(),
            self.config.startup_exe_3.clone(),
            self.config.startup_exe_4.clone(),
        ];
        match save_selected_repo_path_from_startup_executables(&startup_executables, &target_dir) {
            Ok(report) => {
                self.push_history(format!(
                    "selected_repo_path.txt を更新しました: {} <= {} (採用={}, exe={}, 理由={}, 候補数={}, 不採用要約={})",
                    report.output_file.display(),
                    target_dir.display(),
                    report.selected_startup_exe,
                    report.selected_exe_path.display(),
                    report.decision_summary,
                    report.candidate_count,
                    report.rejected_summary
                ));
            }
            Err(err) => {
                self.update_status(format!("selected_repo_path.txt 更新失敗: {err}"));
                self.push_history(format!("selected_repo_path.txt 更新に失敗しました: {err}"));
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

    fn browse_build_root_dir(&mut self) {
        match select_folder_path() {
            Ok(Some(path)) => {
                self.config.build_root_dir = path.clone();
                self.update_status("ビルルートを設定しました");
                self.push_history(format!("ビルルートを参照設定しました: {path}"));
            }
            Ok(None) => {
                self.update_status("ビルルートの参照をキャンセルしました");
            }
            Err(err) => {
                self.update_status(format!("ビルルート参照に失敗: {err}"));
                self.push_history(format!("ビルルート参照に失敗しました: {err}"));
            }
        }
    }

    fn toggle_voice_input(&mut self) {
        self.pending_input_focus = true;
        match send_voice_input_hotkey() {
            Ok(()) => {
                self.voice_input_active = !self.voice_input_active;
                self.update_status(format!(
                    "音声入力ホットキー実行済み: {VOICE_INPUT_HOTKEY_LABEL}"
                ));
                self.push_history(format!(
                    "音声入力ホットキー実行: {} -> {}",
                    VOICE_INPUT_HOTKEY_LABEL,
                    if self.voice_input_active {
                        "読み取り中"
                    } else {
                        "音声入力"
                    }
                ));
            }
            Err(err) => {
                self.update_status(format!("音声入力ホットキー実行失敗: {err}"));
                self.push_history(format!("音声入力ホットキー実行失敗: {err}"));
            }
        }
    }

}
