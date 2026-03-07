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

#[derive(Clone, Debug)]
struct SelectedRepoPathCandidate {
    source_slot: usize,
    source_startup_exe: String,
    resolved_exe_path: PathBuf,
    runtime_dir: PathBuf,
    output_file: PathBuf,
    output_exists: bool,
    runtime_exists: bool,
    score: u32,
    score_reason: String,
}

#[derive(Clone, Debug)]
struct SelectedRepoPathSaveReport {
    selected_startup_exe: String,
    selected_exe_path: PathBuf,
    output_file: PathBuf,
    decision_summary: String,
    candidate_count: usize,
    rejected_summary: String,
}

fn resolve_startup_executable_path(raw: &str, current_dir: &Path) -> Result<PathBuf, String> {
    let trimmed = raw.trim().trim_matches('"');
    if trimmed.is_empty() {
        return Err("空のパスです".to_string());
    }
    let parsed = PathBuf::from(trimmed);
    let resolved = if parsed.is_absolute() {
        parsed
    } else {
        current_dir.join(parsed)
    };
    Ok(resolved)
}

fn collect_selected_repo_path_candidates(
    startup_executables: &[String],
) -> (Vec<SelectedRepoPathCandidate>, Vec<String>) {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut candidates = Vec::new();
    let mut rejected = Vec::new();

    for (index, raw) in startup_executables.iter().enumerate() {
        let slot = index + 1;
        let resolved_exe_path = match resolve_startup_executable_path(raw, &current_dir) {
            Ok(path) => path,
            Err(reason) => {
                rejected.push(format!("startup_exe_{slot}: {reason}"));
                continue;
            }
        };
        let Some(exe_parent) = resolved_exe_path.parent() else {
            rejected.push(format!(
                "startup_exe_{slot}: EXE親ディレクトリを解決できません ({})",
                resolved_exe_path.display()
            ));
            continue;
        };
        let runtime_dir = exe_parent.join("runtime");
        let output_file = runtime_dir.join("selected_repo_path.txt");
        let output_exists = output_file.is_file();
        let runtime_exists = runtime_dir.is_dir();
        let name_hint = resolved_exe_path
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("codex_rollback_bridge");
        if !output_exists && !runtime_exists {
            rejected.push(format!(
                "startup_exe_{slot}: runtime/selected_repo_path.txt が存在しません ({})",
                resolved_exe_path.display()
            ));
            continue;
        }

        let mut score = 0_u32;
        let mut reasons = Vec::new();
        if output_exists {
            score += 100;
            reasons.push("selected_repo_path.txt 既存");
        }
        if runtime_exists {
            score += 10;
            reasons.push("runtime 既存");
        }
        if name_hint {
            score += 1;
            reasons.push("exe名ヒント一致");
        }
        if reasons.is_empty() {
            reasons.push("補助情報なし");
        }

        candidates.push(SelectedRepoPathCandidate {
            source_slot: slot,
            source_startup_exe: raw.trim().to_string(),
            resolved_exe_path,
            runtime_dir,
            output_file,
            output_exists,
            runtime_exists,
            score,
            score_reason: reasons.join(", "),
        });
    }

    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.output_exists.cmp(&left.output_exists))
            .then_with(|| right.runtime_exists.cmp(&left.runtime_exists))
            .then_with(|| left.source_slot.cmp(&right.source_slot))
    });
    (candidates, rejected)
}

fn summarize_rejected_selected_repo_candidates(rejected: &[String]) -> String {
    if rejected.is_empty() {
        return "不採用理由なし".to_string();
    }
    const MAX_ITEMS: usize = 3;
    let mut parts = rejected
        .iter()
        .take(MAX_ITEMS)
        .cloned()
        .collect::<Vec<_>>();
    if rejected.len() > MAX_ITEMS {
        parts.push(format!("他{}件", rejected.len() - MAX_ITEMS));
    }
    parts.join(" / ")
}

fn choose_selected_repo_path_candidate(
    candidates: &[SelectedRepoPathCandidate],
) -> Option<SelectedRepoPathCandidate> {
    candidates.first().cloned()
}

fn validate_selected_repo_write_target(
    candidate: &SelectedRepoPathCandidate,
    target_project_dir_path: &Path,
) -> Result<PathBuf> {
    if target_project_dir_path.as_os_str().is_empty() {
        return Err(anyhow!("target_project_dir_path が空です"));
    }

    let repo_path = if target_project_dir_path.is_absolute() {
        target_project_dir_path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("カレントディレクトリ取得に失敗しました")?
            .join(target_project_dir_path)
    };
    if repo_path.as_os_str().is_empty() {
        return Err(anyhow!("書き込み対象のプロジェクトパス解決後に空になりました"));
    }
    if candidate
        .output_file
        .file_name()
        .and_then(|name| name.to_str())
        != Some("selected_repo_path.txt")
    {
        return Err(anyhow!(
            "書き込み先ファイル名が不正です: {}",
            candidate.output_file.display()
        ));
    }
    let parent = candidate.output_file.parent().ok_or_else(|| {
        anyhow!(
            "selected_repo_path.txt の親ディレクトリを解決できません: {}",
            candidate.output_file.display()
        )
    })?;
    if parent.as_os_str().is_empty() {
        return Err(anyhow!(
            "selected_repo_path.txt の親ディレクトリが空です: {}",
            candidate.output_file.display()
        ));
    }
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "selected_repo_path.txt 用 runtime ディレクトリ作成に失敗: {}",
            parent.display()
        )
    })?;
    let metadata = fs::metadata(parent).with_context(|| {
        format!(
            "selected_repo_path.txt 用 runtime ディレクトリ確認に失敗: {}",
            parent.display()
        )
    })?;
    if !metadata.is_dir() {
        return Err(anyhow!(
            "selected_repo_path.txt の親がディレクトリではありません: {}",
            parent.display()
        ));
    }
    Ok(repo_path)
}

fn save_selected_repo_path_from_startup_executables(
    startup_executables: &[String],
    target_project_dir_path: &Path,
) -> Result<SelectedRepoPathSaveReport> {
    let (candidates, rejected) = collect_selected_repo_path_candidates(startup_executables);
    let rejected_summary = summarize_rejected_selected_repo_candidates(&rejected);
    let candidate_count = candidates.len();
    let candidate = choose_selected_repo_path_candidate(&candidates).ok_or_else(|| {
        anyhow!(
            "selected_repo_path.txt の候補が見つかりません 候補数=0 不採用: {}",
            rejected_summary
        )
    })?;
    let repo_path = validate_selected_repo_write_target(&candidate, target_project_dir_path)?;

    fs::write(&candidate.output_file, format!("{}\n", repo_path.display())).with_context(|| {
        format!(
            "selected_repo_path.txt への書き込みに失敗: {}",
            candidate.output_file.display()
        )
    })?;

    Ok(SelectedRepoPathSaveReport {
        selected_startup_exe: format!("startup_exe_{}={}", candidate.source_slot, candidate.source_startup_exe),
        selected_exe_path: candidate.resolved_exe_path,
        output_file: candidate.output_file,
        decision_summary: format!(
            "score={} [{}] runtime={}",
            candidate.score,
            candidate.score_reason,
            candidate.runtime_dir.display()
        ),
        candidate_count,
        rejected_summary,
    })
}

fn resolve_project_debug_executable_path(declaration_path: &Path) -> Result<PathBuf> {
    let body = fs::read_to_string(declaration_path)
        .with_context(|| format!("宣言ファイル読み込みに失敗: {}", declaration_path.display()))?;
    let line_4 = body
        .lines()
        .nth(3)
        .map(str::trim)
        .ok_or_else(|| anyhow!("宣言ファイルの4行目が見つかりません: {}", declaration_path.display()))?;
    if line_4.is_empty() {
        return Err(anyhow!(
            "宣言ファイルの4行目にEXEパスがありません: {}",
            declaration_path.display()
        ));
    }
    let exe_path = PathBuf::from(line_4.trim_matches('"'));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("codex_shell_{prefix}_{nanos}"))
    }

    #[test]
    fn save_selected_repo_path_single_runtime_candidate() {
        let root = test_dir("single");
        let exe_dir = root.join("tool").join("target").join("debug");
        let runtime_dir = exe_dir.join("runtime");
        fs::create_dir_all(&runtime_dir).expect("runtime dir");
        let exe_path = exe_dir.join("tool.exe");
        fs::write(&exe_path, "").expect("exe file");
        let target_dir = root.join("project_a");
        fs::create_dir_all(&target_dir).expect("target dir");

        let report = save_selected_repo_path_from_startup_executables(
            &[exe_path.to_string_lossy().into_owned()],
            &target_dir,
        )
        .expect("save report");
        assert_eq!(report.output_file, runtime_dir.join("selected_repo_path.txt"));
        let body = fs::read_to_string(&report.output_file).expect("output body");
        assert_eq!(body, format!("{}\n", target_dir.display()));
    }

    #[test]
    fn prefer_candidate_with_existing_selected_repo_file() {
        let root = test_dir("prefer_existing");
        let exe_a_dir = root.join("a").join("target").join("debug");
        let exe_b_dir = root.join("b").join("target").join("debug");
        fs::create_dir_all(exe_a_dir.join("runtime")).expect("runtime a");
        fs::create_dir_all(exe_b_dir.join("runtime")).expect("runtime b");
        let exe_a = exe_a_dir.join("app_a.exe");
        let exe_b = exe_b_dir.join("app_b.exe");
        fs::write(&exe_a, "").expect("exe a");
        fs::write(&exe_b, "").expect("exe b");
        let b_output = exe_b_dir.join("runtime").join("selected_repo_path.txt");
        fs::write(&b_output, "previous\n").expect("existing output");
        let target_dir = root.join("project_b");
        fs::create_dir_all(&target_dir).expect("target dir");

        let report = save_selected_repo_path_from_startup_executables(
            &[
                exe_a.to_string_lossy().into_owned(),
                exe_b.to_string_lossy().into_owned(),
            ],
            &target_dir,
        )
        .expect("save report");
        assert_eq!(report.output_file, b_output);
        assert!(report.decision_summary.contains("selected_repo_path.txt 既存"));
    }

    #[test]
    fn reject_bridge_name_without_runtime() {
        let root = test_dir("bridge_no_runtime");
        let exe_dir = root
            .join("codex_rollback_bridge")
            .join("target")
            .join("debug");
        fs::create_dir_all(&exe_dir).expect("exe dir");
        let exe_path = exe_dir.join("codex_rollback_bridge.exe");
        fs::write(&exe_path, "").expect("exe file");
        let target_dir = root.join("project_c");
        fs::create_dir_all(&target_dir).expect("target dir");

        let err = save_selected_repo_path_from_startup_executables(
            &[exe_path.to_string_lossy().into_owned()],
            &target_dir,
        )
        .expect_err("must fail");
        assert!(
            err.to_string().contains("候補が見つかりません"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn support_relative_startup_executable_path() {
        let _guard = test_lock().lock().expect("lock");
        let root = test_dir("relative");
        let old_dir = std::env::current_dir().expect("current dir");
        fs::create_dir_all(root.join("tools").join("runtime")).expect("runtime dir");
        fs::write(root.join("tools").join("relative_tool.exe"), "").expect("exe file");
        let target_dir = root.join("project_d");
        fs::create_dir_all(&target_dir).expect("target dir");
        std::env::set_current_dir(&root).expect("set current dir");

        let result = save_selected_repo_path_from_startup_executables(
            &[r"tools\relative_tool.exe".to_string()],
            &target_dir,
        );
        std::env::set_current_dir(old_dir).expect("restore current dir");
        let report = result.expect("save report");
        assert_eq!(
            report.output_file,
            root.join("tools").join("runtime").join("selected_repo_path.txt")
        );
    }

    #[test]
    fn fail_with_rejection_summary_when_all_candidates_invalid() {
        let root = test_dir("all_invalid");
        let target_dir = root.join("project_e");
        fs::create_dir_all(&target_dir).expect("target dir");
        let err = save_selected_repo_path_from_startup_executables(
            &["".to_string(), r".\missing\tool.exe".to_string()],
            &target_dir,
        )
        .expect_err("must fail");
        let text = err.to_string();
        assert!(text.contains("候補数=0"), "unexpected error: {text}");
        assert!(text.contains("不採用"), "unexpected error: {text}");
    }
}

