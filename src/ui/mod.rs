pub mod panels;
pub mod peripherals;

use std::time::Duration;

use crate::app::{CentralTab, EmuApp, LeftTab};
use eframe::egui;

pub fn render_layout(app: &mut EmuApp, ui: &mut egui::Ui) {
    let mut step_clicked = false;

    // MOBILE CODE
    let is_mobile = ui.ctx().content_rect().width() < 800.0;

    // TOP TOOLBAR
    egui::Panel::top("top_panel").show_inside(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
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
            // End of File Menu

            // 2. VIEW MENU (New Zoom Controls)
            ui.menu_button("View", |ui| {
                let current_zoom = ui.ctx().zoom_factor();

                if ui.button("🔍 Zoom In").clicked() {
                    ui.ctx().set_zoom_factor(current_zoom * 1.2);
                }
                if ui.button("🔍 Zoom Out").clicked() {
                    ui.ctx().set_zoom_factor(current_zoom / 1.2);
                }
                ui.separator();
                if ui.button("🔄 Reset Zoom").clicked() {
                    ui.ctx().set_zoom_factor(1.0);
                }

                // Optional: Show current scale percentage
                ui.label(
                    egui::RichText::new(format!("Current: {:.0}%", current_zoom * 100.0)).weak(),
                );
            });

            ui.separator();

            // MOBILE CODE: Display if not mobile
            if !is_mobile {
                ui.heading("HyperEmu Emulator");
                ui.separator();
            }

            egui::ComboBox::from_id_salt("arch_combo")
                .selected_text(app.current_backend().name())
                .show_ui(ui, |ui| {
                    let mut idx = None;
                    for (i, backend) in app.backends.iter().enumerate() {
                        if ui
                            .selectable_label(app.active_backend == i, backend.name())
                            .clicked()
                        {
                            if app.active_backend != i {
                                idx = Some(i);
                            }
                        }
                    }
                    if let Some(idx) = idx {
                        app.switch_backend(idx);
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
                        app.unconsumed_time = Duration::ZERO;

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

                ui.separator();

                ui.add_sized(
                    egui::vec2(200.0, ui.available_height()),
                    egui::Slider::new(&mut app.clock_speed.0, 1..=16_000_000)
                        .logarithmic(true)
                        .text("Speed")
                        // 1. Format the number into a beautiful string for the UI
                        .custom_formatter(|n, _| {
                            if n >= 1_000_000.0 {
                                format!("{:.2} MHz", n / 1_000_000.0)
                            } else if n >= 1_000.0 {
                                format!("{:.1} kHz", n / 1_000.0)
                            } else {
                                format!("{} Hz", n as u64)
                            }
                        })
                        // 2. Parse the user's custom string back into a raw number
                        .custom_parser(|s| {
                            let s = s.trim().to_lowercase();
                            // Strip "hz" if they typed it
                            let s = s.strip_suffix("hz").unwrap_or(&s).trim();

                            let mut multiplier = 1.0;
                            let mut num_str = s;

                            // Check for 'k' (kilo) or 'm' (mega)
                            if let Some(stripped) = s.strip_suffix('k') {
                                multiplier = 1_000.0;
                                num_str = stripped.trim();
                            } else if let Some(stripped) = s.strip_suffix('m') {
                                multiplier = 1_000_000.0;
                                num_str = stripped.trim();
                            }

                            // Multiply the float and return it!
                            num_str.parse::<f64>().ok().map(|n| n * multiplier)
                        }),
                );
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

    // MOBILE CODE: If mobile, render a completely different layout with a single panel and a nav bar
    if is_mobile {
        // MOBILE LAYOUT (Single Panel + Nav Bar)
        egui::Panel::top("mobile_nav").show_inside(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(
                    &mut app.mobile_tab,
                    crate::app::MobileTab::Editor,
                    "📝 Code",
                );
                ui.selectable_value(&mut app.mobile_tab, crate::app::MobileTab::Cpu, "🧠 CPU");
                ui.selectable_value(
                    &mut app.mobile_tab,
                    crate::app::MobileTab::Hardware,
                    "💡 Hardware",
                );
                ui.selectable_value(
                    &mut app.mobile_tab,
                    crate::app::MobileTab::Consoles,
                    "🖥 Consoles",
                );
                ui.selectable_value(
                    &mut app.mobile_tab,
                    crate::app::MobileTab::Memory,
                    "💾 Memory",
                );
            });
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            if let Some(err) = &app.error_msg {
                ui.colored_label(egui::Color32::RED, err);
                ui.separator();
            }

            egui::ScrollArea::vertical().show(ui, |ui| match app.mobile_tab {
                crate::app::MobileTab::Editor => panels::render_editor(ui, app),
                crate::app::MobileTab::Cpu => {
                    panels::render_cpu(ui, app);
                    ui.separator();
                    panels::render_stack(ui, app);
                }
                crate::app::MobileTab::Hardware => panels::render_dynamic_gpios(ui, app),
                crate::app::MobileTab::Consoles => panels::render_consoles(ui, app),
                crate::app::MobileTab::Memory => {
                    panels::render_memory_map(ui, app);
                    ui.separator();
                    panels::render_memory_view(ui, app);
                }
            });
        });
    } else {
        // DESKTOP LAYOUT (Three Panels)
        let max_panel_width = ui.ctx().content_rect().width() * 0.35;

        // LEFT PANEL (Hardware, Consoles, Memory Map)
        egui::Panel::left("left_panel")
            .min_size(250.0)
            .max_size(max_panel_width)
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
            .max_size(max_panel_width)
            .resizable(true)
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("right_scroll")
                    .show(ui, |ui| {
                        panels::render_cpu(ui, app);

                        ui.add_space(20.0);

                        panels::render_stack(ui, app);
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
}
