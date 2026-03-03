#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct AppConfig {
    working_dir: String,
    build_command: String,
    codex_command: String,
    pipe_name: String,
    input_prefix: String,
    startup_exe_1: String,
    startup_exe_2: String,
    startup_exe_3: String,
    startup_exe_4: String,
    show_size_overlay: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_string()),
            build_command: DEFAULT_BUILD_COMMAND.to_string(),
            codex_command: DEFAULT_CODEX_COMMAND.to_string(),
            pipe_name: DEFAULT_PIPE_NAME.to_string(),
            input_prefix: String::new(),
            startup_exe_1: String::new(),
            startup_exe_2: String::new(),
            startup_exe_3: String::new(),
            startup_exe_4: String::new(),
            show_size_overlay: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiDefinition {
    version: u32,
    assets: UiAssets,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    objects: Vec<UiObject>,
    screens: Vec<UiScreen>,
}

impl Default for UiDefinition {
    fn default() -> Self {
        Self {
            version: 1,
            assets: UiAssets::default(),
            objects: Vec::new(),
            screens: Vec::new(),
        }
    }
}

impl UiDefinition {
    fn normalize_screens(&mut self) {
        let legacy_objects = std::mem::take(&mut self.objects);
        if self.screens.is_empty() {
            self.screens.push(UiScreen {
                id: UI_MAIN_SCREEN_ID.to_string(),
                objects: legacy_objects,
            });
        } else if !legacy_objects.is_empty() {
            if let Some(main_screen_index) = self.screen_index(UI_MAIN_SCREEN_ID) {
                if self.screens[main_screen_index].objects.is_empty() {
                    self.screens[main_screen_index].objects = legacy_objects;
                } else {
                    self.objects.clear();
                }
            } else {
                self.screens.push(UiScreen {
                    id: UI_MAIN_SCREEN_ID.to_string(),
                    objects: legacy_objects,
                });
            }
        }

        if self.screen(UI_MAIN_SCREEN_ID).is_none() {
            self.screens.push(UiScreen {
                id: UI_MAIN_SCREEN_ID.to_string(),
                objects: Vec::new(),
            });
        }
        if self.screen(UI_SETTINGS_SCREEN_ID).is_none() {
            self.screens.push(default_settings_screen());
        }
        self.objects.clear();
    }

    fn screen_ids(&self) -> Vec<String> {
        self.screens.iter().map(|screen| screen.id.clone()).collect()
    }

    fn screen(&self, screen_id: &str) -> Option<&UiScreen> {
        self.screens.iter().find(|screen| screen.id == screen_id)
    }

    fn screen_index(&self, screen_id: &str) -> Option<usize> {
        self.screens.iter().position(|screen| screen.id == screen_id)
    }

    fn screen_mut(&mut self, screen_id: &str) -> Option<&mut UiScreen> {
        self.screens
            .iter_mut()
            .find(|screen| screen.id == screen_id)
    }

    fn object_index_in_screen(&self, screen_id: &str, object_id: &str) -> Option<usize> {
        self.screen(screen_id)?
            .objects
            .iter()
            .position(|object| object.id == object_id)
    }

    fn screen_objects(&self, screen_id: &str) -> Option<&Vec<UiObject>> {
        Some(&self.screen(screen_id)?.objects)
    }

    fn screen_objects_mut(&mut self, screen_id: &str) -> Option<&mut Vec<UiObject>> {
        Some(&mut self.screen_mut(screen_id)?.objects)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct UiScreen {
    id: String,
    objects: Vec<UiObject>,
}

impl Default for UiScreen {
    fn default() -> Self {
        Self {
            id: String::new(),
            objects: Vec::new(),
        }
    }
}

fn ensure_project_target_label(definition: &mut UiDefinition) {
    let Some(objects) = definition.screen_objects_mut(UI_MAIN_SCREEN_ID) else {
        return;
    };
    if objects
        .iter()
        .any(|object| object.id == PROJECT_TARGET_LABEL_ID)
    {
        return;
    }

    let start_rect = objects
        .iter()
        .find(|object| object.id == "btn_codex_start")
        .map(|object| (object.position.y, object.size.h));
    let input_rect = objects
        .iter()
        .find(|object| object.id == "input_command")
        .map(|object| (object.position.x, object.position.y, object.size.w));

    let height = 24.0;
    let x = input_rect.map_or(24.0, |(x, _, _)| x);
    let width = input_rect.map_or(780.0, |(_, _, width)| width.max(220.0));
    let y = match (start_rect, input_rect) {
        (Some((start_y, start_h)), Some((_, input_y, _))) => {
            let start_bottom = start_y + start_h;
            let mut candidate = start_bottom + 6.0;
            if candidate + height > input_y {
                candidate = ((start_bottom + input_y - height) / 2.0).max(start_bottom);
            }
            candidate
        }
        _ => 56.0,
    };

    let mut object = create_label_object(
        PROJECT_TARGET_LABEL_ID,
        "プロジェクト無し",
        45,
        x,
        y,
        width,
        height,
        "left",
    );
    object.bind.command = ui_tool::PROJECT_TARGET_STATE.to_string();
    objects.push(object);
}

fn default_settings_screen() -> UiScreen {
    let mut objects = vec![
        create_label_object("lbl_settings_title", "設定", 100, 24.0, 18.0, 240.0, 28.0, "left"),
        create_label_object(
            "lbl_settings_working_dir",
            "起動フォルダ",
            100,
            24.0,
            64.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_working_dir",
            ui_tool::CONFIG_WORKING_DIR,
            110,
            156.0,
            64.0,
            640.0,
            24.0,
        ),
        create_label_object("lbl_settings_build", "ビルド", 100, 24.0, 96.0, 120.0, 24.0, "left"),
        create_input_object(
            "input_settings_build",
            ui_tool::CONFIG_BUILD_COMMAND,
            110,
            156.0,
            96.0,
            640.0,
            24.0,
        ),
        create_label_object("lbl_settings_codex", "Codex", 100, 24.0, 128.0, 120.0, 24.0, "left"),
        create_input_object(
            "input_settings_codex",
            ui_tool::CONFIG_CODEX_COMMAND,
            110,
            156.0,
            128.0,
            640.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_pipe_name",
            "パイプ名",
            100,
            24.0,
            160.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_pipe_name",
            ui_tool::CONFIG_PIPE_NAME,
            110,
            156.0,
            160.0,
            640.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_input_prefix",
            "入力先頭付加",
            100,
            24.0,
            192.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_input_prefix",
            ui_tool::CONFIG_INPUT_PREFIX,
            110,
            156.0,
            192.0,
            640.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_1",
            "自動起動EXE1",
            100,
            24.0,
            224.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_1",
            ui_tool::CONFIG_STARTUP_EXE_1,
            110,
            156.0,
            224.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_1_browse",
            "参照",
            ui_tool::CONFIG_STARTUP_EXE_1_BROWSE,
            120,
            804.0,
            224.0,
            72.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_2",
            "自動起動EXE2",
            100,
            24.0,
            252.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_2",
            ui_tool::CONFIG_STARTUP_EXE_2,
            110,
            156.0,
            252.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_2_browse",
            "参照",
            ui_tool::CONFIG_STARTUP_EXE_2_BROWSE,
            120,
            804.0,
            252.0,
            72.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_3",
            "自動起動EXE3",
            100,
            24.0,
            280.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_3",
            ui_tool::CONFIG_STARTUP_EXE_3,
            110,
            156.0,
            280.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_3_browse",
            "参照",
            ui_tool::CONFIG_STARTUP_EXE_3_BROWSE,
            120,
            804.0,
            280.0,
            72.0,
            24.0,
        ),
        create_label_object(
            "lbl_settings_startup_exe_4",
            "自動起動EXE4",
            100,
            24.0,
            308.0,
            120.0,
            24.0,
            "left",
        ),
        create_input_object(
            "input_settings_startup_exe_4",
            ui_tool::CONFIG_STARTUP_EXE_4,
            110,
            156.0,
            308.0,
            640.0,
            24.0,
        ),
        create_button_object(
            "btn_settings_startup_exe_4_browse",
            "参照",
            ui_tool::CONFIG_STARTUP_EXE_4_BROWSE,
            120,
            804.0,
            308.0,
            72.0,
            24.0,
        ),
        create_checkbox_object(
            "chk_settings_show_size_overlay",
            "サイズ表示を表示",
            ui_tool::CONFIG_SHOW_SIZE_OVERLAY,
            110,
            24.0,
            336.0,
            280.0,
            28.0,
        ),
        create_button_object(
            "btn_settings_save",
            "設定保存",
            ui_tool::CONFIG_SAVE,
            120,
            24.0,
            368.0,
            120.0,
            28.0,
        ),
        create_button_object(
            "btn_settings_restart",
            "PowerShell再起動",
            ui_tool::CONFIG_RESTART_LISTENER,
            120,
            152.0,
            368.0,
            180.0,
            28.0,
        ),
        create_button_object(
            "btn_settings_back",
            "閉じる",
            ui_tool::NAV_BACK_MAIN,
            120,
            340.0,
            368.0,
            120.0,
            28.0,
        ),
        create_checkbox_object(
            "chk_settings_ui_edit",
            "UI編集",
            ui_tool::UI_EDIT_TOGGLE,
            130,
            468.0,
            368.0,
            120.0,
            28.0,
        ),
    ];
    for object in &mut objects {
        if object.id == "chk_settings_ui_edit" {
            object.checked = false;
        }
    }
    UiScreen {
        id: UI_SETTINGS_SCREEN_ID.to_string(),
        objects,
    }
}
