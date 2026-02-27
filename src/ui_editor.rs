use crate::{UI_RELOAD_CHECK_INTERVAL_MS, UiDefinition};
use eframe::egui::{self, Color32, RichText};
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
    ui_has_unsaved_changes: bool,
) -> UiEditorEvents {
    let mut events = UiEditorEvents::default();

    let viewport_id = egui::ViewportId::from_hash_of("ui_editor_viewport");
    let builder = egui::ViewportBuilder::default()
        .with_title("UI編集")
        .with_inner_size([360.0, 520.0])
        .with_min_inner_size([320.0, 420.0])
        .with_resizable(true)
        .with_close_button(true);

    ctx.show_viewport_immediate(viewport_id, builder, |editor_ctx, viewport_class| {
        if editor_ctx.input(|input| input.viewport().close_requested()) {
            events.closed = true;
            return;
        }

        let panel_events = if viewport_class == egui::ViewportClass::Embedded {
            let mut inner_events = UiEditorPanelEvents::default();
            egui::Window::new("UI編集")
                .default_width(340.0)
                .resizable(true)
                .show(editor_ctx, |ui| {
                    inner_events = render_ui_editor_contents(
                        ui,
                        ui_definition,
                        ui_selected_screen_id,
                        ui_selected_object_id,
                        ui_has_unsaved_changes,
                    );
                });
            inner_events
        } else {
            let mut inner_events = UiEditorPanelEvents::default();
            egui::CentralPanel::default().show(editor_ctx, |ui| {
                inner_events = render_ui_editor_contents(
                    ui,
                    ui_definition,
                    ui_selected_screen_id,
                    ui_selected_object_id,
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
    ui_has_unsaved_changes: bool,
) -> UiEditorPanelEvents {
    let mut events = UiEditorPanelEvents::default();

    ui.label(RichText::new("オブジェクトをドラッグすると位置を変更できます").color(Color32::BLACK));
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
    }

    egui::ComboBox::from_label("対象画面")
        .selected_text(ui_selected_screen_id.clone())
        .show_ui(ui, |ui| {
            for screen_id in &screen_ids {
                if ui
                    .selectable_label(ui_selected_screen_id == screen_id, screen_id)
                    .clicked()
                {
                    *ui_selected_screen_id = screen_id.clone();
                    ui_selected_object_id.clear();
                }
            }
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
    if ui_selected_object_id.is_empty()
        || ui_definition
            .object_index_in_screen(ui_selected_screen_id, ui_selected_object_id)
            .is_none()
    {
        *ui_selected_object_id = screen_objects[0].id.clone();
    }
    ui.label(
        RichText::new(format!("総オブジェクト数: {}", screen_objects.len())).color(Color32::BLACK),
    );

    let mut ordered_objects: Vec<(usize, String, i32)> = screen_objects
        .iter()
        .enumerate()
        .map(|(index, object)| (index, object.id.clone(), object.z_index))
        .collect();
    ordered_objects.sort_by(|left, right| left.2.cmp(&right.2).then(left.0.cmp(&right.0)));

    egui::ComboBox::from_label("対象オブジェクト")
        .selected_text(ui_selected_object_id.clone())
        .show_ui(ui, |ui| {
            for (order, (_index, object_id, z_index)) in ordered_objects.iter().enumerate() {
                ui.selectable_value(
                    ui_selected_object_id,
                    object_id.clone(),
                    format!("{}: {} (z={})", order + 1, object_id, z_index),
                );
            }
        });

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

    events
}

fn is_radio_object_type(object_type: &str) -> bool {
    matches!(object_type.trim(), "radio" | "radio_button")
}
