use crate::{UI_MAIN_SCREEN_ID, UI_RELOAD_CHECK_INTERVAL_MS, UiDefinition, UiObject};
use eframe::egui::{self, Color32, RichText};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::time::Duration;

#[derive(Default)]
pub(crate) struct UiEditorEvents {
    pub(crate) changed: bool,
    pub(crate) save_requested: bool,
    pub(crate) closed: bool,
}

#[derive(Default)]
struct UiEditorPanelEvents {
    changed: bool,
    save_requested: bool,
}

pub(crate) fn render_ui_editor_viewport(
    ctx: &egui::Context,
    ui_definition: &mut UiDefinition,
    ui_selected_screen_id: &mut String,
    ui_selected_object_id: &mut String,
    ui_selected_object_ids: &mut Vec<String>,
    ui_edit_grid_visible: &mut bool,
    ui_font_names: &[String],
    show_size_overlay: bool,
    main_window_size: egui::Vec2,
    ui_has_unsaved_changes: bool,
) -> UiEditorEvents {
    let mut events = UiEditorEvents::default();

    let viewport_id = egui::ViewportId::from_hash_of("ui_editor_viewport");
    let default_position = ctx
        .input(|input| input.viewport().outer_rect)
        .map(|rect| egui::pos2(rect.max.x + 12.0, rect.min.y));
    let mut builder = egui::ViewportBuilder::default()
        .with_title("UI編集")
        .with_inner_size([360.0, 715.0])
        .with_min_inner_size([320.0, 470.0])
        .with_resizable(true)
        .with_close_button(true);
    if let Some(position) = default_position {
        builder = builder.with_position(position);
    }

    ctx.show_viewport_immediate(viewport_id, builder, |editor_ctx, viewport_class| {
        if editor_ctx.input(|input| input.viewport().close_requested()) {
            events.closed = true;
            return;
        }

        let panel_events = if viewport_class == egui::ViewportClass::Embedded {
            let mut inner_events = UiEditorPanelEvents::default();
            let created_window_size = editor_ctx.content_rect().size();
            egui::Window::new("UI編集")
                .default_width(340.0)
                .resizable(true)
                .show(editor_ctx, |ui| {
                    inner_events = render_ui_editor_contents(
                        ui,
                        ui_definition,
                        ui_selected_screen_id,
                        ui_selected_object_id,
                        ui_selected_object_ids,
                        ui_edit_grid_visible,
                        ui_font_names,
                        show_size_overlay,
                        main_window_size,
                        created_window_size,
                        ui_has_unsaved_changes,
                    );
                });
            inner_events
        } else {
            let mut inner_events = UiEditorPanelEvents::default();
            let created_window_size = editor_ctx.content_rect().size();
            egui::CentralPanel::default().show(editor_ctx, |ui| {
                inner_events = render_ui_editor_contents(
                    ui,
                    ui_definition,
                    ui_selected_screen_id,
                    ui_selected_object_id,
                    ui_selected_object_ids,
                    ui_edit_grid_visible,
                    ui_font_names,
                    show_size_overlay,
                    main_window_size,
                    created_window_size,
                    ui_has_unsaved_changes,
                );
            });
            inner_events
        };

        events.changed |= panel_events.changed;
        events.save_requested |= panel_events.save_requested;
        editor_ctx.request_repaint_after(Duration::from_millis(UI_RELOAD_CHECK_INTERVAL_MS));
    });

    events
}

fn render_ui_editor_contents(
    ui: &mut egui::Ui,
    ui_definition: &mut UiDefinition,
    ui_selected_screen_id: &mut String,
    ui_selected_object_id: &mut String,
    ui_selected_object_ids: &mut Vec<String>,
    ui_edit_grid_visible: &mut bool,
    ui_font_names: &[String],
    show_size_overlay: bool,
    main_window_size: egui::Vec2,
    created_window_size: egui::Vec2,
    ui_has_unsaved_changes: bool,
) -> UiEditorPanelEvents {
    let mut events = UiEditorPanelEvents::default();

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let unsaved_text = if ui_has_unsaved_changes {
            "未保存の変更があります"
        } else {
            "保存済み"
        };
        ui.label(RichText::new(unsaved_text).color(Color32::BLACK));
        if ui
            .add_enabled(ui_has_unsaved_changes, egui::Button::new("保存"))
            .clicked()
        {
            events.save_requested = true;
        }
    });
    ui.add_space(6.0);

    let screen_ids = ui_definition.screen_ids();
    if screen_ids.is_empty() {
        ui.label(RichText::new("画面がありません").color(Color32::BLACK));
        return events;
    }
    if ui_selected_screen_id.is_empty()
        || ui_definition.screen(ui_selected_screen_id.as_str()).is_none()
    {
        *ui_selected_screen_id = screen_ids[0].clone();
        ui_selected_object_id.clear();
        ui_selected_object_ids.clear();
    }

    ui.horizontal(|ui| {
        ui.label(RichText::new("対象画面").color(Color32::BLACK));
        egui::ComboBox::from_id_salt("ui_editor_target_screen")
            .selected_text(ui_selected_screen_id.clone())
            .show_ui(ui, |ui| {
                for screen_id in &screen_ids {
                    if ui
                        .selectable_label(ui_selected_screen_id == screen_id, screen_id)
                        .clicked()
                    {
                        *ui_selected_screen_id = screen_id.clone();
                        ui_selected_object_id.clear();
                        ui_selected_object_ids.clear();
                    }
                }
            });
        });
    ui.add_space(6.0);

    let screen_objects = match ui_definition.screen_objects(ui_selected_screen_id.as_str()) {
        Some(objects) => objects,
        None => {
            ui.label(RichText::new("選択画面が見つかりません").color(Color32::BLACK));
            return events;
        }
    };

    if screen_objects.is_empty() {
        ui.label(RichText::new("オブジェクトがありません").color(Color32::BLACK));
        return events;
    }
    let size_overlay_target = if show_size_overlay {
        Some(resolve_target_window_size(
            ui_selected_screen_id.as_str(),
            screen_objects,
            main_window_size,
            created_window_size,
        ))
    } else {
        None
    };
    if ui_selected_object_id.is_empty()
        || ui_definition
            .object_index_in_screen(ui_selected_screen_id, ui_selected_object_id)
            .is_none()
    {
        *ui_selected_object_id = screen_objects[0].id.clone();
        ui_selected_object_ids.clear();
        ui_selected_object_ids.push(ui_selected_object_id.clone());
    } else if !ui_selected_object_ids
        .iter()
        .any(|selected_id| selected_id == ui_selected_object_id)
    {
        ui_selected_object_ids.insert(0, ui_selected_object_id.clone());
    }
    ui.label(
        RichText::new(format!("総オブジェクト数: {}", screen_objects.len())).color(Color32::BLACK),
    );

    let mut ordered_objects: Vec<(usize, String, i32, String)> = screen_objects
        .iter()
        .enumerate()
        .map(|(index, object)| {
            let fallback = format!("{}: {} (z={})", index + 1, object.id, object.z_index);
            let list_text = if object.visual.text.value.trim().is_empty() {
                fallback
            } else {
                object.visual.text.value.clone()
            };
            (index, object.id.clone(), object.z_index, list_text)
        })
        .collect();
    ordered_objects.sort_by(|left, right| left.2.cmp(&right.2).then(left.0.cmp(&right.0)));

    ui.horizontal(|ui| {
        ui.label(RichText::new("対象オブジェクト").color(Color32::BLACK));
        let selected_object_text = ordered_objects
            .iter()
            .find(|(_index, object_id, _z_index, _list_text)| object_id == ui_selected_object_id)
            .map(|(_index, _object_id, _z_index, list_text)| list_text.clone())
            .unwrap_or_else(|| ui_selected_object_id.clone());
        egui::ComboBox::from_id_salt("ui_editor_target_object")
            .selected_text(selected_object_text)
            .show_ui(ui, |ui| {
                for (_order, (_index, object_id, _z_index, list_text)) in
                    ordered_objects.iter().enumerate()
                {
                    ui.selectable_value(
                        ui_selected_object_id,
                        object_id.clone(),
                        list_text.clone(),
                    );
                }
            });
        });
    if !ui_selected_object_ids
        .iter()
        .any(|selected_id| selected_id == ui_selected_object_id)
    {
        ui_selected_object_ids.clear();
        ui_selected_object_ids.push(ui_selected_object_id.clone());
    }

    if let Some(index) =
        ui_definition.object_index_in_screen(ui_selected_screen_id, ui_selected_object_id)
    {
        let screen_objects = ui_definition
            .screen_objects_mut(ui_selected_screen_id.as_str())
            .expect("selected screen should exist");
        let object = &mut screen_objects[index];
        let mut changed = false;

        ui.label(RichText::new(format!("種別: {}", object.object_type)).color(Color32::BLACK));
        changed |= ui.checkbox(&mut object.visible, "表示").changed();
        ui.horizontal(|ui| {
            ui.label("フォント");
            changed |= ui
                .add(egui::DragValue::new(&mut object.visual.text.font_size).speed(0.5))
                .changed();
        });
        if object.visual.text.font_family.trim().is_empty() {
            object.visual.text.font_family = "noto_sans_jp".to_string();
            changed = true;
        }
        ui.horizontal(|ui| {
            ui.label("フォント選択");
            let current_font = object.visual.text.font_family.clone();
            egui::ComboBox::from_id_salt("ui_editor_font_family")
                .selected_text(current_font)
                .show_ui(ui, |ui| {
                    for font_name in ui_font_names {
                        ui.selectable_value(
                            &mut object.visual.text.font_family,
                            font_name.clone(),
                            font_name,
                        );
                    }
                });
        });
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut object.visual.text.bold, "太文字").changed();
            changed |= ui.checkbox(&mut object.visual.text.italic, "斜め").changed();
        });
        ui.horizontal(|ui| {
            ui.label("寄せ");
            changed |= ui
                .selectable_value(&mut object.visual.text.align, "left".to_string(), "左寄せ")
                .changed();
            changed |= ui
                .selectable_value(&mut object.visual.text.align, "center".to_string(), "中央")
                .changed();
            changed |= ui
                .selectable_value(&mut object.visual.text.align, "right".to_string(), "右寄せ")
                .changed();
        });
        if object.visual.text.font_size < 1.0 {
            object.visual.text.font_size = 1.0;
            changed = true;
        }
        changed |= ui.checkbox(&mut object.enabled, "有効").changed();
        if matches!(object.object_type.trim(), "checkbox" | "radio" | "radio_button") {
            changed |= ui.checkbox(&mut object.checked, "チェック状態").changed();
        }
        if is_radio_object_type(&object.object_type) {
            ui.label(RichText::new("ラジオグループ").color(Color32::BLACK));
            changed |= ui.text_edit_singleline(&mut object.bind.group).changed();
        }

        ui.horizontal(|ui| {
            ui.label("座標X");
            changed |= ui
                .add(egui::DragValue::new(&mut object.position.x).speed(1.0))
                .changed();
            ui.label("座標Y");
            changed |= ui
                .add(egui::DragValue::new(&mut object.position.y).speed(1.0))
                .changed();
        });

        ui.horizontal(|ui| {
            ui.label("幅");
            changed |= ui
                .add(egui::DragValue::new(&mut object.size.w).speed(1.0))
                .changed();
            ui.label("高さ");
            changed |= ui
                .add(egui::DragValue::new(&mut object.size.h).speed(1.0))
                .changed();
        });
        ui.horizontal(|ui| {
            ui.label("表示順");
            changed |= ui
                .add(egui::DragValue::new(&mut object.z_index).speed(1.0))
                .changed();
        });

        ui.label(RichText::new("表示テキスト").color(Color32::BLACK));
        changed |= ui.text_edit_singleline(&mut object.visual.text.value).changed();
        ui.label(RichText::new("対象オブジェクト名").color(Color32::BLACK));
        ui.horizontal(|ui| {
            ui.add_enabled(false, egui::TextEdit::singleline(&mut object.id));
            if ui.button("コピー").clicked() {
                ui.ctx().copy_text(object.id.clone());
            }
        });

        ui.label(RichText::new("背景画像キー").color(Color32::BLACK));
        changed |= ui
            .text_edit_singleline(&mut object.visual.background.image)
            .changed();

        ui.label(RichText::new("背景フィット").color(Color32::BLACK));
        changed |= ui
            .text_edit_singleline(&mut object.visual.background.fit)
            .changed();

        events.changed = changed;
    } else {
        ui.label(RichText::new("選択オブジェクトが見つかりません").color(Color32::BLACK));
    }

    let mut align_changed = false;
    ui.separator();
    if let Some(target_window_size) = size_overlay_target {
        let win_x = target_window_size.x.max(0.0).round() as i32;
        let win_y = target_window_size.y.max(0.0).round() as i32;
        ui.label(RichText::new("対象ウィンドウサイズ").color(Color32::BLACK));
        ui.label(RichText::new(format!("x={win_x} y={win_y}")).color(Color32::BLACK));
        ui.separator();
    }
    ui.label(RichText::new("整列").color(Color32::BLACK));
    ui.checkbox(ui_edit_grid_visible, "グリッド表示 (10px / 50px太線)");
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("縦系").color(Color32::BLACK));
        let enabled = ui_selected_object_ids.len() >= 2;
        if ui.add_enabled(enabled, egui::Button::new("上揃え")).clicked() {
            align_changed |= apply_alignment(
                ui_definition,
                ui_selected_screen_id.as_str(),
                ui_selected_object_ids,
                AlignMode::Top,
            );
        }
        if ui.add_enabled(enabled, egui::Button::new("下揃え")).clicked() {
            align_changed |= apply_alignment(
                ui_definition,
                ui_selected_screen_id.as_str(),
                ui_selected_object_ids,
                AlignMode::Bottom,
            );
        }
        if ui.add_enabled(enabled, egui::Button::new("中央揃え")).clicked() {
            align_changed |= apply_alignment(
                ui_definition,
                ui_selected_screen_id.as_str(),
                ui_selected_object_ids,
                AlignMode::MiddleVertical,
            );
        }
        if ui.add_enabled(enabled, egui::Button::new("等間隔")).clicked() {
            align_changed |= apply_alignment(
                ui_definition,
                ui_selected_screen_id.as_str(),
                ui_selected_object_ids,
                AlignMode::DistributeVertical,
            );
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("横系").color(Color32::BLACK));
        let enabled = ui_selected_object_ids.len() >= 2;
        if ui.add_enabled(enabled, egui::Button::new("左揃え")).clicked() {
            align_changed |= apply_alignment(
                ui_definition,
                ui_selected_screen_id.as_str(),
                ui_selected_object_ids,
                AlignMode::Left,
            );
        }
        if ui.add_enabled(enabled, egui::Button::new("右揃え")).clicked() {
            align_changed |= apply_alignment(
                ui_definition,
                ui_selected_screen_id.as_str(),
                ui_selected_object_ids,
                AlignMode::Right,
            );
        }
        if ui.add_enabled(enabled, egui::Button::new("中央揃え")).clicked() {
            align_changed |= apply_alignment(
                ui_definition,
                ui_selected_screen_id.as_str(),
                ui_selected_object_ids,
                AlignMode::MiddleHorizontal,
            );
        }
        if ui.add_enabled(enabled, egui::Button::new("等間隔")).clicked() {
            align_changed |= apply_alignment(
                ui_definition,
                ui_selected_screen_id.as_str(),
                ui_selected_object_ids,
                AlignMode::DistributeHorizontal,
            );
        }
    });
    events.changed |= align_changed;

    events
}
