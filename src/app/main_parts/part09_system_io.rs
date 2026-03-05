fn unix_timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs().to_string(),
        Err(_) => "0".to_string(),
    }
}

fn send_voice_input_hotkey() -> Result<()> {
    app::process_ops::send_voice_input_hotkey()
}

fn find_project_declaration_files(base_dir: &Path) -> Result<Vec<PathBuf>> {
    if !base_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let root_entries = fs::read_dir(base_dir)
        .with_context(|| format!("起動フォルダ走査に失敗: {}", base_dir.display()))?;
    for root_entry in root_entries.flatten() {
        let dir_path = root_entry.path();
        if !dir_path.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir_path) else {
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name.starts_with(PROJECT_DECLARATION_PREFIX)
                && name.ends_with(PROJECT_DECLARATION_SUFFIX)
            {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn read_project_name_from_declaration(path: &Path) -> Option<String> {
    let body = fs::read_to_string(path).ok()?;
    let first_line = body.lines().next()?.trim();
    if first_line.is_empty() {
        None
    } else {
        Some(first_line.to_string())
    }
}

fn resolve_project_debug_executable_path(declaration_path: &Path, build_root_dir: &Path) -> Result<PathBuf> {
    let project_dir = declaration_path
        .parent()
        .ok_or_else(|| anyhow!("宣言ファイルの親フォルダを取得できません: {}", declaration_path.display()))?;
    let folder_name = project_dir
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| anyhow!("プロジェクトフォルダ名を取得できません: {}", project_dir.display()))?;
    let exe_path = build_root_dir
        .join(folder_name)
        .join("debug")
        .join(format!("{folder_name}.exe"));
    if !exe_path.is_file() {
        return Err(anyhow!(
            "debug実行ファイルが見つかりません: {}",
            exe_path.display()
        ));
    }
    Ok(exe_path)
}

fn format_system_time_hhmm(system_time: SystemTime) -> Option<String> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

        let since_unix = system_time.duration_since(UNIX_EPOCH).ok()?;
        let unix_100ns = since_unix
            .as_secs()
            .checked_mul(10_000_000)?
            .checked_add((since_unix.subsec_nanos() / 100) as u64)?;
        let windows_epoch_offset_100ns = 11644473600_u64.checked_mul(10_000_000)?;
        let filetime_ticks = unix_100ns.checked_add(windows_epoch_offset_100ns)?;
        let file_time = FILETIME {
            dwLowDateTime: filetime_ticks as u32,
            dwHighDateTime: (filetime_ticks >> 32) as u32,
        };
        let mut utc_time = SYSTEMTIME::default();
        if unsafe { FileTimeToSystemTime(&file_time, &mut utc_time) }.is_err() {
            return None;
        }
        let mut local_time = SYSTEMTIME::default();
        if unsafe { SystemTimeToTzSpecificLocalTime(None, &utc_time, &mut local_time) }.is_err() {
            return None;
        }
        return Some(format!("{:02}:{:02}", local_time.wHour, local_time.wMinute));
    }

    #[cfg(not(windows))]
    {
        let since_unix = system_time.duration_since(UNIX_EPOCH).ok()?;
        let total_minutes = (since_unix.as_secs() / 60) % (24 * 60);
        let hour = total_minutes / 60;
        let minute = total_minutes % 60;
        Some(format!("{hour:02}:{minute:02}"))
    }
}

fn resolve_project_debug_launch_target(exe_path: &Path) -> PathBuf {
    let shortcut_candidate = exe_path.with_extension("lnk");
    if shortcut_candidate.is_file() {
        shortcut_candidate
    } else {
        exe_path.to_path_buf()
    }
}

fn launch_target_with_shell(target: &Path, working_dir: &Path) -> Result<()> {
    let status = Command::new("cmd")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg("/D")
        .arg(working_dir)
        .arg(target)
        .status()
        .with_context(|| format!("シェル起動に失敗: {}", target.display()))?;
    if !status.success() {
        return Err(anyhow!(
            "シェル起動が失敗しました status={}: {}",
            status,
            target.display()
        ));
    }
    Ok(())
}

fn load_config() -> Result<AppConfig> {
    let path = config_file_path();
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let config_text = fs::read_to_string(&path)
        .with_context(|| format!("設定ファイル読み込みに失敗: {}", path.display()))?;
    let config: AppConfig = serde_json::from_str(&config_text)
        .with_context(|| format!("設定ファイル解析に失敗: {}", path.display()))?;
    Ok(config)
}

fn save_config(config: &AppConfig) -> Result<()> {
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("設定ディレクトリ作成に失敗: {}", parent.display()))?;
    }

    let body = serde_json::to_string_pretty(config).context("設定シリアライズに失敗")?;
    fs::write(&path, format!("{body}\n"))
        .with_context(|| format!("設定ファイル保存に失敗: {}", path.display()))?;
    Ok(())
}

fn config_base_dir() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("com", "gonec", "codex-shell") {
        return project_dirs.config_dir().to_path_buf();
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn config_file_path() -> PathBuf {
    config_base_dir().join("config.json")
}

#[allow(dead_code)]
fn listener_script_path() -> PathBuf {
    config_base_dir().join(LISTENER_FILE_NAME)
}

fn asset_base_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current_dir) = std::env::current_dir() {
        candidates.push(current_dir);
    }

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        candidates.push(exe_dir.to_path_buf());
        if let Some(parent) = exe_dir.parent() {
            candidates.push(parent.to_path_buf());
        }
    }

    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn ui_runtime_base_dir() -> PathBuf {
    for candidate in asset_base_candidates() {
        if candidate.join(UI_LIVE_RELATIVE_PATH).is_file() {
            return candidate;
        }
    }
    if let Ok(current_dir) = std::env::current_dir() {
        return current_dir;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn ui_live_file_path() -> PathBuf {
    ui_runtime_base_dir().join(UI_LIVE_RELATIVE_PATH)
}

fn ensure_live_ui_file() -> Result<PathBuf> {
    let live_path = ui_live_file_path();

    if !live_path.is_file() {
        return Err(anyhow!("live UI定義が見つかりません: {}", live_path.display()));
    }

    Ok(live_path)
}

fn load_ui_definition(path: &Path) -> Result<UiDefinition> {
    let body = fs::read_to_string(path)
        .with_context(|| format!("UI定義読み込みに失敗: {}", path.display()))?;
    let mut definition: UiDefinition = serde_json::from_str(&body)
        .with_context(|| format!("UI定義解析に失敗: {}", path.display()))?;
    definition.normalize_screens();
    Ok(definition)
}

fn save_ui_definition(path: &Path, definition: &UiDefinition) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("UI定義ディレクトリ作成に失敗: {}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(definition).context("UI定義シリアライズに失敗")?;
    fs::write(path, format!("{body}\n"))
        .with_context(|| format!("UI定義保存に失敗: {}", path.display()))?;
    Ok(())
}

fn ui_file_modified_time(path: &Path) -> Result<SystemTime> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .with_context(|| format!("UI定義更新時刻取得に失敗: {}", path.display()))
}

fn required_asset_path(relative_path: &str) -> Result<PathBuf> {
    let candidates = asset_base_candidates()
        .into_iter()
        .map(|base| base.join(relative_path))
        .collect::<Vec<_>>();

    for path in &candidates {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
    }

    let tried = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    Err(anyhow!(
        "必須ファイルが見つかりません: {relative_path} / tried: {tried}"
    ))
}

fn apply_required_font(ctx: &egui::Context) -> Result<(PathBuf, Vec<String>)> {
    let font_path = required_asset_path(FONT_RELATIVE_PATH)?;
    let _ofl_path = required_asset_path(FONT_OFL_RELATIVE_PATH)?;
    let _source_path = required_asset_path(FONT_SOURCE_RELATIVE_PATH)?;
    let font_dir = font_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("フォントディレクトリ解決に失敗: {}", font_path.display()))?;
    let mut loaded_fonts: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in fs::read_dir(&font_dir)
        .with_context(|| format!("フォントディレクトリ読み込みに失敗: {}", font_dir.display()))?
    {
        let entry = match entry {
            Ok(value) => value,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if !matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let key = stem.replace(' ', "_");
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        loaded_fonts.push((key, bytes));
    }
    if !loaded_fonts.iter().any(|(name, _)| name == "noto_sans_jp") {
        let font_bytes = fs::read(&font_path)
            .with_context(|| format!("フォント読み込みに失敗: {}", font_path.display()))?;
        loaded_fonts.insert(0, ("noto_sans_jp".to_string(), font_bytes));
    }
    loaded_fonts.sort_by(|left, right| left.0.cmp(&right.0));
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.clear();
    fonts.families.clear();
    let mut font_names = Vec::new();
    for (font_name, font_bytes) in loaded_fonts {
        if !fonts.font_data.contains_key(&font_name) {
            fonts.font_data.insert(
                font_name.clone(),
                Arc::new(egui::FontData::from_owned(font_bytes)),
            );
            font_names.push(font_name);
        }
    }
    if font_names.is_empty() {
        return Err(anyhow!(
            "フォントが見つかりません: {}",
            font_dir.to_string_lossy()
        ));
    }
    fonts
        .families
        .insert(egui::FontFamily::Proportional, font_names.clone());
    fonts
        .families
        .insert(egui::FontFamily::Monospace, font_names.clone());
    for font_name in &font_names {
        fonts.families.insert(
            egui::FontFamily::Name(Arc::from(font_name.clone())),
            vec![font_name.clone()],
        );
    }
    ctx.set_fonts(fonts);
    Ok((font_path, font_names))
}

fn apply_visual_fix(ctx: &egui::Context) {
    let base_text = Color32::from_rgb(0, 0, 0);
    let strong_text = Color32::from_rgb(0, 0, 0);
    let weak_text = Color32::from_rgb(24, 24, 24);
    let panel_bg = Color32::WHITE;
    let button_border = Color32::from_rgb(0, 0, 0);

    ctx.set_theme(egui::Theme::Light);
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.visuals.dark_mode = false;
        style.visuals.text_alpha_from_coverage = egui::epaint::AlphaFromCoverage::Gamma(0.55);
        style.visuals.disabled_alpha = 1.0;

        style.visuals.override_text_color = Some(base_text);
        style.visuals.weak_text_color = Some(weak_text);
        style.visuals.widgets.noninteractive.fg_stroke.color = base_text;
        style.visuals.widgets.inactive.fg_stroke.color = base_text;
        style.visuals.widgets.hovered.fg_stroke.color = strong_text;
        style.visuals.widgets.active.fg_stroke.color = strong_text;
        style.visuals.widgets.open.fg_stroke.color = strong_text;

        style.visuals.panel_fill = panel_bg;
        style.visuals.window_fill = panel_bg;
        style.visuals.faint_bg_color = Color32::from_gray(250);
        style.visuals.extreme_bg_color = Color32::WHITE;
        style.visuals.widgets.noninteractive.bg_fill = panel_bg;
        style.visuals.widgets.inactive.bg_fill = Color32::from_gray(248);
        style.visuals.widgets.hovered.bg_fill = Color32::from_gray(240);
        style.visuals.widgets.active.bg_fill = Color32::from_gray(232);
        style.visuals.widgets.open.bg_fill = Color32::from_gray(232);

        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(2.0, button_border);
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(2.0, button_border);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(2.0, button_border);
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, button_border);
        style.visuals.widgets.open.bg_stroke = egui::Stroke::new(2.0, button_border);
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
        style.visuals.widgets.open.corner_radius = egui::CornerRadius::same(4);
    });
}

#[allow(dead_code)]
fn write_listener_script(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "待ち受けスクリプトディレクトリ作成に失敗: {}",
                parent.display()
            )
        })?;
    }

    let mut script_bytes = Vec::with_capacity(3 + LISTENER_SCRIPT.len());
    script_bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    script_bytes.extend_from_slice(LISTENER_SCRIPT.as_bytes());

    fs::write(path, script_bytes)
        .with_context(|| format!("待ち受けスクリプト保存に失敗: {}", path.display()))?;
    Ok(())
}

fn spawn_listener_process(
    pipe_name: &str,
    working_dir: &str,
    window_title: &str,
) -> Result<Child> {
    let current_exe =
        std::env::current_exe().context("自身の実行ファイルパス取得に失敗しました")?;
    let mut command = Command::new(&current_exe);
    command
        .arg("--conpty-listener")
        .arg("--pipe-name")
        .arg(pipe_name.trim())
        .arg("--working-directory")
        .arg(working_dir.trim())
        .arg("--window-title")
        .arg(window_title);
    #[cfg(windows)]
    {
        command.creation_flags(CREATE_NEW_CONSOLE_FLAG);
    }
    let child = command
        .spawn()
        .with_context(|| {
            format!(
                "ConPTY待ち受け起動に失敗: exe={} pipe={}",
                current_exe.display(),
                pipe_name.trim()
            )
        })?;
    Ok(child)
}

