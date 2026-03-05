impl CodexShellApp {

    fn render_runtime_header(&mut self, _ctx: &egui::Context) {
    }

    fn render_build_confirm_dialog(&mut self, ctx: &egui::Context) {
        if !self.build_confirm_open {
            return;
        }
        if self.input_command.trim().is_empty() {
            self.cancel_build_when_empty();
            return;
        }

        let mut open = true;
        egui::Window::new("ビルド確認")
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .fixed_size(egui::vec2(360.0, 132.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(RichText::new("ビルドを実行しますか？").color(Color32::BLACK));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("はい").clicked() {
                        if self.input_command.trim().is_empty() {
                            self.cancel_build_when_empty();
                        } else {
                            self.build_confirm_open = false;
                            self.push_history("ビルド確認: はい");
                            self.send_build_command();
                        }
                    }
                    if ui.button("いいえ").clicked() {
                        self.build_confirm_open = false;
                        self.update_status("ビルドをキャンセルしました");
                        self.push_history("ビルド確認: いいえ");
                    }
                });
            });

        if !open && self.build_confirm_open {
            self.build_confirm_open = false;
            self.update_status("ビルドをキャンセルしました");
            self.push_history("ビルド確認ダイアログを閉じました");
        }
    }

    fn set_primary_selected_object(&mut self, object_id: String) {
        self.ui_selected_object_id = object_id.clone();
        self.ui_selected_object_ids.clear();
        if !object_id.is_empty() {
            self.ui_selected_object_ids.push(object_id);
        }
    }

    fn ensure_selected_objects_valid(&mut self, screen_id: &str) {
        let Some(objects) = self.ui_definition.screen_objects(screen_id) else {
            self.ui_selected_object_id.clear();
            self.ui_selected_object_ids.clear();
            return;
        };

        self.ui_selected_object_ids
            .retain(|selected_id| objects.iter().any(|object| object.id == *selected_id));

        if self.ui_selected_object_id.is_empty()
            || !objects
                .iter()
                .any(|object| object.id == self.ui_selected_object_id)
        {
            if let Some(first_selected_id) = self.ui_selected_object_ids.first() {
                self.ui_selected_object_id = first_selected_id.clone();
            } else {
                self.ui_selected_object_id = objects
                    .first()
                    .map(|object| object.id.clone())
                    .unwrap_or_default();
            }
        }

        if self.ui_selected_object_id.is_empty() {
            self.ui_selected_object_ids.clear();
            return;
        }

        if let Some(primary_position) = self
            .ui_selected_object_ids
            .iter()
            .position(|selected_id| selected_id == &self.ui_selected_object_id)
        {
            if primary_position != 0 {
                let primary_id = self.ui_selected_object_ids.remove(primary_position);
                self.ui_selected_object_ids.insert(0, primary_id);
            }
        } else {
            self.ui_selected_object_ids
                .insert(0, self.ui_selected_object_id.clone());
        }
    }

    fn render_obj_container_or_group(&self, ctx: &mut RenderObjCtx<'_>) {
        let fill = if ctx.object.visual.background.image.trim().is_empty() {
            Color32::from_gray(250)
        } else {
            Color32::from_gray(242)
        };
        egui::Frame::default()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, Color32::BLACK))
            .inner_margin(egui::Margin::same(4))
            .show(ctx.ui, |ui| {
                ui.set_min_size(ctx.object_size);
            });
    }

    fn render_obj_label(&self, ctx: &mut RenderObjCtx<'_>) {
        let text = self.resolve_object_text(ctx.object);
        let main_align = match ctx.object.visual.text.align.trim() {
            "left" => egui::Align::Min,
            "right" => egui::Align::Max,
            _ => egui::Align::Center,
        };
        let mut rich = RichText::new(text)
            .font(ctx.text_font.clone())
            .color(self.resolve_label_color(ctx.object));
        if ctx.object.visual.text.bold {
            rich = rich.strong();
        }
        if ctx.object.visual.text.italic {
            rich = rich.italics();
        }
        ctx.ui.allocate_ui_with_layout(
            ctx.object_size,
            egui::Layout::left_to_right(egui::Align::Center).with_main_align(main_align),
            |ui| {
                ui.add(egui::Label::new(rich).selectable(false).sense(egui::Sense::hover()));
            },
        );
    }

    fn render_obj_input(&mut self, ctx: &mut RenderObjCtx<'_>, state_changed: &mut bool) {
        let enabled = ctx.controls_enabled && ctx.object.enabled;
        match ctx.object_command {
            ui_tool::CONFIG_WORKING_DIR => {
                let response = ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [ctx.object_size.x, ctx.object_size.y],
                        TextEdit::singleline(&mut self.config.working_dir),
                    )
                });
                if response.inner.changed() {
                    *state_changed = true;
                }
            }
            ui_tool::CONFIG_BUILD_COMMAND => {
                let response = ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [ctx.object_size.x, ctx.object_size.y],
                        TextEdit::singleline(&mut self.config.build_command),
                    )
                });
                if response.inner.changed() {
                    *state_changed = true;
                }
            }
            ui_tool::CONFIG_CODEX_COMMAND | ui_tool::CONFIG_CODEX_COMMAND_A => {
                let response = ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [ctx.object_size.x, ctx.object_size.y],
                        TextEdit::singleline(&mut self.config.codex_command_a),
                    )
                });
                if response.inner.changed() {
                    *state_changed = true;
                }
            }
            ui_tool::CONFIG_CODEX_COMMAND_B => {
                let response = ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [ctx.object_size.x, ctx.object_size.y],
                        TextEdit::singleline(&mut self.config.codex_command_b),
                    )
                });
                if response.inner.changed() {
                    *state_changed = true;
                }
            }
            ui_tool::CONFIG_INPUT_PREFIX => {
                let response = ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [ctx.object_size.x, ctx.object_size.y],
                        TextEdit::singleline(&mut self.config.input_prefix),
                    )
                });
                if response.inner.changed() {
                    *state_changed = true;
                }
            }
            ui_tool::CONFIG_STARTUP_EXE_1 => {
                let response = ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [ctx.object_size.x, ctx.object_size.y],
                        TextEdit::singleline(&mut self.config.startup_exe_1),
                    )
                });
                if response.inner.changed() {
                    *state_changed = true;
                }
            }
            ui_tool::CONFIG_STARTUP_EXE_2 => {
                let response = ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [ctx.object_size.x, ctx.object_size.y],
                        TextEdit::singleline(&mut self.config.startup_exe_2),
                    )
                });
                if response.inner.changed() {
                    *state_changed = true;
                }
            }
            ui_tool::CONFIG_STARTUP_EXE_3 => {
                let response = ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [ctx.object_size.x, ctx.object_size.y],
                        TextEdit::singleline(&mut self.config.startup_exe_3),
                    )
                });
                if response.inner.changed() {
                    *state_changed = true;
                }
            }
            ui_tool::CONFIG_STARTUP_EXE_4 => {
                let response = ctx.ui.add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        [ctx.object_size.x, ctx.object_size.y],
                        TextEdit::singleline(&mut self.config.startup_exe_4),
                    )
                });
                if response.inner.changed() {
                    *state_changed = true;
                }
            }
            _ => {
                let input_font_id = egui::FontId::monospace(INPUT_FONT_SIZE);
                let row_height = ctx.ui.fonts_mut(|fonts| fonts.row_height(&input_font_id));
                let desired_rows = ((ctx.object_size.y - FIXED_INPUT_HEIGHT_PADDING).max(row_height)
                    / row_height)
                    .floor()
                    .max(1.0) as usize;
                let frame_stroke = if ctx.object_id == "input_command" {
                    egui::Stroke::NONE
                } else {
                    egui::Stroke::new(1.0, Color32::BLACK)
                };
                let frame_fill = if ctx.object_id == "input_command" {
                    Color32::from_gray(242)
                } else {
                    Color32::WHITE
                };
                let input_response = egui::Frame::default()
                    .fill(frame_fill)
                    .stroke(frame_stroke)
                    .inner_margin(egui::Margin::same(4))
                    .show(ctx.ui, |ui| {
                        let input_line_count = if ctx.object_id == "input_command" {
                            self.input_command.chars().filter(|ch| *ch == '\n').count() + 1
                        } else {
                            1
                        };
                        let mut editor = TextEdit::multiline(&mut self.input_command)
                            .id_source(INPUT_COMMAND_ID_SALT)
                            .font(input_font_id)
                            .interactive(enabled)
                            .desired_width(f32::INFINITY)
                            .desired_rows(desired_rows);
                        if ctx.object_id == "input_command" {
                            let ime_commit_this_frame = ui.input(|input| {
                                input.events.iter().any(|event| {
                                    matches!(event, egui::Event::Ime(egui::ImeEvent::Commit(_)))
                                })
                            });
                            let input_return_key = if ime_commit_this_frame {
                                None
                            } else {
                                Some(egui::KeyboardShortcut::new(
                                    egui::Modifiers::NONE,
                                    egui::Key::Enter,
                                ))
                            };
                            editor = editor.frame(false).return_key(input_return_key);
                            let visible_height = (ctx.object_size.y - 8.0).max(1.0);
                            let editor_height = ((input_line_count.max(desired_rows) as f32)
                                * row_height
                                + FIXED_INPUT_HEIGHT_PADDING)
                                .max(visible_height);
                            return egui::ScrollArea::vertical()
                                .id_salt("input_command_vertical_scroll")
                                .auto_shrink([false, false])
                                .max_height(visible_height)
                                .show(ui, |ui| {
                                    ui.add_sized(
                                        [(ctx.object_size.x - 8.0).max(1.0), editor_height],
                                        editor,
                                    )
                                })
                                .inner;
                        }
                        ui.add_sized(
                            [
                                (ctx.object_size.x - 8.0).max(1.0),
                                (ctx.object_size.y - 8.0).max(1.0),
                            ],
                            editor,
                        )
                    });
                if enabled && self.pending_input_focus {
                    input_response.inner.request_focus();
                    self.pending_input_focus = false;
                }
                self.input_area_size = input_response.response.rect.size();
            }
        }
    }

    fn render_obj_image(&self, ctx: &mut RenderObjCtx<'_>) {
        let image_key = ctx.object.visual.background.image.trim();
        let text = if image_key.is_empty() {
            "image".to_string()
        } else {
            format!("image: {image_key}")
        };
        egui::Frame::default()
            .fill(Color32::from_gray(245))
            .stroke(egui::Stroke::new(1.0, Color32::BLACK))
            .inner_margin(egui::Margin::same(4))
            .show(ctx.ui, |ui| {
                ui.set_min_size(ctx.object_size);
                ui.label(RichText::new(text).color(Color32::BLACK));
            });
    }

    fn render_obj_project_combo_box(&mut self, ctx: &mut RenderObjCtx<'_>) {
        let enabled = ctx.controls_enabled
            && ctx.object.enabled
            && self.is_bind_command_enabled(ctx.object_command);
        let highlight_green = self.is_selected_project_highlighted();
        let placeholder_text = ctx.object.visual.text.value.trim();
        let selected_text = self
            .project_selected_index
            .and_then(|index| self.project_declarations.get(index))
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| {
                if placeholder_text.is_empty() {
                    "プロジェクトを選択".to_string()
                } else {
                    placeholder_text.to_string()
                }
            });
        let mut selected_index = self.project_selected_index;
        ctx.ui.add_enabled_ui(enabled, |ui| {
            ui.allocate_ui_with_layout(
                ctx.object_size,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.scope(|ui| {
                        let fixed_width = ctx.object_size.x.max(12.0);
                        if highlight_green {
                            let green = Color32::from_rgb(188, 233, 188);
                            let open_green = Color32::from_rgb(172, 224, 172);
                            let visuals = &mut ui.style_mut().visuals;
                            visuals.widgets.noninteractive.weak_bg_fill = green;
                            visuals.widgets.noninteractive.bg_fill = green;
                            visuals.widgets.inactive.weak_bg_fill = green;
                            visuals.widgets.inactive.bg_fill = green;
                            visuals.widgets.hovered.weak_bg_fill = green;
                            visuals.widgets.hovered.bg_fill = green;
                            visuals.widgets.active.weak_bg_fill = open_green;
                            visuals.widgets.active.bg_fill = open_green;
                            visuals.widgets.open.weak_bg_fill = open_green;
                            visuals.widgets.open.bg_fill = open_green;
                        }
                        ui.style_mut()
                            .text_styles
                            .insert(egui::TextStyle::Button, ctx.text_font.clone());
                        ui.style_mut()
                            .text_styles
                            .insert(egui::TextStyle::Body, ctx.text_font.clone());
                        ui.spacing_mut().combo_width = fixed_width;
                        ui.spacing_mut().interact_size.y = ctx.object_size.y.max(18.0);
                        ui.set_min_width(fixed_width);
                        ui.set_max_width(fixed_width);
                        egui::ComboBox::from_id_salt(("project_combo_box", ctx.object_id))
                            .width(fixed_width)
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                if self.project_declarations.is_empty() {
                                    ui.label("プロジェクト宣言_*.md が見つかりません");
                                } else {
                                    for (index, entry) in self.project_declarations.iter().enumerate()
                                    {
                                        ui.selectable_value(
                                            &mut selected_index,
                                            Some(index),
                                            entry.name.as_str(),
                                        );
                                    }
                                }
                            });
                    });
                },
            );
        });
        if selected_index != self.project_selected_index {
            self.project_selected_index = selected_index;
            self.sync_selected_project_target_dir();
        }
    }

    fn render_obj_checkbox(&self, ctx: &mut RenderObjCtx<'_>) -> Option<bool> {
        let text = self.resolve_object_text(ctx.object);
        let enabled = ctx.controls_enabled
            && ctx.object.enabled
            && self.is_bind_command_enabled(ctx.object_command);
        let mut checked = self
            .runtime_checked_for_command(ctx.object_command)
            .unwrap_or(ctx.object.checked);
        let mut rich = RichText::new(text).font(ctx.text_font.clone());
        if ctx.object.visual.text.bold {
            rich = rich.strong();
        }
        if ctx.object.visual.text.italic {
            rich = rich.italics();
        }
        let response = ctx.ui.add_enabled_ui(enabled, |ui| {
            ui.add_sized(
                [ctx.object_size.x, ctx.object_size.y],
                egui::Checkbox::new(&mut checked, rich),
            )
        });
        if response.inner.changed() {
            Some(checked)
        } else {
            None
        }
    }

    fn render_obj_radio(&self, ctx: &mut RenderObjCtx<'_>) -> bool {
        let text = self.resolve_object_text(ctx.object);
        let enabled = ctx.controls_enabled
            && ctx.object.enabled
            && self.is_bind_command_enabled(ctx.object_command);
        let checked = self
            .runtime_checked_for_command(ctx.object_command)
            .unwrap_or(ctx.object.checked);
        let mut rich = RichText::new(text).font(ctx.text_font.clone());
        if ctx.object.visual.text.bold {
            rich = rich.strong();
        }
        if ctx.object.visual.text.italic {
            rich = rich.italics();
        }
        let response = ctx.ui.add_enabled_ui(enabled, |ui| {
            ui.add_sized(
                [ctx.object_size.x, ctx.object_size.y],
                egui::RadioButton::new(checked, rich),
            )
        });
        response.inner.clicked() && !checked
    }

    fn render_obj_button(&self, ctx: &mut RenderObjCtx<'_>) -> bool {
        let text = self.resolve_object_text(ctx.object);
        let disabled_for_selected_project = ctx.object_id == "btn_project_target_move"
            && self.is_selected_project_highlighted();
        let codex_a_running = ctx.object_command == ui_tool::MODE_CODEX_START
            && self.codex_runtime_state == CodexRuntimeState::Calculating;
        let codex_b_running = ctx.object_command == ui_tool::MODE_CODEX_START_B
            && self.codex_runtime_state_b == CodexRuntimeState::Calculating;
        let highlight_orange = codex_a_running || codex_b_running;
        let enabled = ctx.controls_enabled
            && ctx.object.enabled
            && self.is_bind_command_enabled(ctx.object_command)
            && !disabled_for_selected_project;
        let mut rich = RichText::new(text).font(ctx.text_font.clone());
        if ctx.object.visual.text.bold {
            rich = rich.strong();
        }
        if ctx.object.visual.text.italic {
            rich = rich.italics();
        }
        if !enabled && !highlight_orange {
            rich = rich.color(Color32::from_gray(140));
        }
        let response = ctx.ui.scope(|ui| {
            if highlight_orange {
                let orange = Color32::from_rgb(255, 192, 120);
                let orange_active = Color32::from_rgb(247, 176, 92);
                let visuals = &mut ui.style_mut().visuals;
                visuals.widgets.noninteractive.weak_bg_fill = orange;
                visuals.widgets.noninteractive.bg_fill = orange;
                visuals.widgets.inactive.weak_bg_fill = orange;
                visuals.widgets.inactive.bg_fill = orange;
                visuals.widgets.hovered.weak_bg_fill = orange;
                visuals.widgets.hovered.bg_fill = orange;
                visuals.widgets.active.weak_bg_fill = orange_active;
                visuals.widgets.active.bg_fill = orange_active;
                visuals.widgets.open.weak_bg_fill = orange_active;
                visuals.widgets.open.bg_fill = orange_active;
            }
            ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized([ctx.object_size.x, ctx.object_size.y], egui::Button::new(rich))
            })
        });
        response.inner.inner.clicked()
    }

    fn render_obj_by_type(
        &mut self,
        ctx: &mut RenderObjCtx<'_>,
        state_changed: &mut bool,
        clicked: &mut bool,
        checkbox_changed: &mut Option<bool>,
        radio_selected: &mut bool,
    ) {
        match ctx.object_type {
            "panel" => self.render_obj_container_or_group(ctx),
            "label" => self.render_obj_label(ctx),
            "input" => self.render_obj_input(ctx, state_changed),
            "image" => self.render_obj_image(ctx),
            "combo_box" | "combobox" | "project_dropdown" | "dropdown" => {
                self.render_obj_project_combo_box(ctx)
            }
            "checkbox" => {
                *checkbox_changed = self.render_obj_checkbox(ctx);
            }
            "radio" | "radio_button" => {
                *radio_selected = self.render_obj_radio(ctx);
            }
            _ => {
                *clicked = self.render_obj_button(ctx);
            }
        }
    }

}
