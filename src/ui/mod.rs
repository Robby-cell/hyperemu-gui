pub mod panels;
pub mod peripherals;

use crate::app::{CentralTab, EmuApp, LeftTab};
use eframe::egui;

pub fn render_layout(app: &mut EmuApp, ui: &mut egui::Ui) {
    let mut step_clicked = false;

    // TOP TOOLBAR
    egui::Panel::top("top_panel").show_inside(ui, |ui| {
        ui.horizontal(|ui| {
            // Add the File Menu here
            ui.menu_button("File", |ui| {
                if ui.button("📂 Load Workspace...").clicked() {
                    app.trigger_load();
                    ui.close();
                }
                if ui.button("💾 Save Workspace As...").clicked() {
                    app.trigger_save();
                    ui.close();
                }
            });
            ui.separator();
            // End of File Menu

            ui.heading("HyperEmu Emulator");
            ui.separator();

            egui::ComboBox::from_id_salt("arch_combo")
                .selected_text(app.current_backend().name())
                .show_ui(ui, |ui| {
                    for (i, backend) in app.backends.iter().enumerate() {
                        if ui
                            .selectable_label(app.active_backend == i, backend.name())
                            .clicked()
                        {
                            app.active_backend = i;
                            if app.code.trim().is_empty() {
                                app.code = backend.default_code().to_string();
                            }
                        }
                    }
                });

            ui.separator();

            if ui.button("⚙ Compile & Load").clicked() {
                panels::compile_and_load(app);
            }

            if app.emu.is_some() {
                ui.separator();
                if app.is_running {
                    if ui.button("⏸ Pause").clicked() {
                        app.is_running = false;
                    }
                } else {
                    if ui.button("▶ Run").clicked() {
                        app.is_running = true;
                        let pc_reg = app.current_backend().pc_reg();
                        if let Some(emu) = &app.emu {
                            *app.ignore_next_bp.lock().unwrap() =
                                Some(emu.reg_read(pc_reg).unwrap_or(0) as u64);
                        }
                    }
                    if ui.button("⏭ Step").clicked() {
                        step_clicked = true;
                        let pc_reg = app.current_backend().pc_reg();
                        if let Some(emu) = &app.emu {
                            *app.ignore_next_bp.lock().unwrap() =
                                Some(emu.reg_read(pc_reg).unwrap_or(0) as u64);
                        }
                    }
                }
            }
        });
    });

    if step_clicked {
        app.snapshot_registers();
        if let Some(emu) = &mut app.emu {
            if let Err(e) = emu.step() {
                app.error_msg = Some(format!("Step Error: {:?}", e));
            }
        }
    }

    // LEFT PANEL (Hardware, Consoles, Memory Map)
    egui::Panel::left("left_panel")
        .min_size(320.0)
        .resizable(true)
        .show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.left_tab, LeftTab::Hardware, "Hardware");
                ui.selectable_value(&mut app.left_tab, LeftTab::Consoles, "Consoles");
                ui.selectable_value(&mut app.left_tab, LeftTab::MemoryMap, "Memory Map");
            });
            ui.separator();

            match app.left_tab {
                LeftTab::Hardware => panels::render_dynamic_gpios(ui, app),
                LeftTab::Consoles => panels::render_consoles(ui, app),
                LeftTab::MemoryMap => panels::render_memory_map(ui, app),
            }
        });

    // RIGHT PANEL (CPU Registers pinned for debugging)
    egui::Panel::right("right_panel")
        .min_size(250.0)
        .resizable(true)
        .show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("right_scroll")
                .show(ui, |ui| {
                    panels::render_cpu(ui, app);
                });
        });

    // CENTRAL PANEL (Code Editor, Disassembly, Memory View)
    egui::CentralPanel::default().show_inside(ui, |ui| {
        if let Some(err) = &app.error_msg {
            ui.colored_label(egui::Color32::RED, err);
            ui.separator();
        }

        ui.horizontal(|ui| {
            ui.selectable_value(&mut app.central_tab, CentralTab::Editor, "Editor");
            ui.selectable_value(&mut app.central_tab, CentralTab::Disassembly, "Disassembly");
            ui.selectable_value(&mut app.central_tab, CentralTab::MemoryView, "Memory View");
        });
        ui.separator();

        match app.central_tab {
            CentralTab::Editor => panels::render_editor(ui, app),
            CentralTab::Disassembly => panels::render_disassembly(ui, app),
            CentralTab::MemoryView => panels::render_memory_view(ui, app),
        }
    });
}
