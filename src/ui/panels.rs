use crate::app::{DeviceType, EmuApp, MemMapRecord};
use crate::backend::ArchBackend;
use crate::ui::peripherals::{GuiUartWriter, PeripheralCategory, UartGui};
use eframe::egui;
use hyperemu::HyperEmu;
use hyperemu::bus::{MemoryBus, Perms};
use hyperemu::device::{ram::Ram, uart::Uart};
use hyperemu::interface::Cpu;
use std::sync::{Arc, Mutex};

fn append_highlighted(
    backend: &dyn ArchBackend,
    job: &mut egui::text::LayoutJob,
    text: &str,
    bg: egui::Color32,
    font: egui::FontId,
) {
    let mut word_start = 0;
    let mut in_word = false;

    let highlight_word =
        |job: &mut egui::text::LayoutJob, word: &str, bg: egui::Color32, font: egui::FontId| {
            let color = if word.ends_with(':') || word.starts_with('.') {
                egui::Color32::from_rgb(220, 220, 170)
            } else if backend.is_instruction(word) {
                egui::Color32::from_rgb(197, 134, 192)
            } else if backend.is_register(word) {
                egui::Color32::from_rgb(78, 201, 176)
            } else if word.starts_with('#') || word.parse::<i32>().is_ok() || word.starts_with("0x")
            {
                egui::Color32::from_rgb(181, 206, 168)
            } else {
                egui::Color32::from_rgb(212, 212, 212)
            };
            job.append(
                word,
                0.0,
                egui::text::TextFormat {
                    font_id: font,
                    color,
                    background: bg,
                    ..Default::default()
                },
            );
        };

    for (i, c) in text.char_indices() {
        if c == '@' || c == ';' {
            if in_word {
                highlight_word(job, &text[word_start..i], bg, font.clone());
            }
            job.append(
                &text[i..],
                0.0,
                egui::text::TextFormat {
                    font_id: font,
                    color: egui::Color32::from_rgb(106, 153, 85),
                    background: bg,
                    ..Default::default()
                },
            );
            return;
        }

        let is_word_char = c.is_alphanumeric() || c == '_' || c == '#' || c == '.' || c == ':';

        if is_word_char && !in_word {
            word_start = i;
            in_word = true;
        } else if !is_word_char && in_word {
            highlight_word(job, &text[word_start..i], bg, font.clone());
            in_word = false;
        }

        if !is_word_char {
            let end_idx = i + c.len_utf8();
            job.append(
                &text[i..end_idx],
                0.0,
                egui::text::TextFormat {
                    font_id: font.clone(),
                    color: egui::Color32::LIGHT_GRAY,
                    background: bg,
                    ..Default::default()
                },
            );
        }
    }

    if in_word {
        highlight_word(job, &text[word_start..], bg, font);
    }
}

pub fn compile_and_load(app: &mut EmuApp) {
    app.is_running = false;
    app.error_msg = None;
    app.prev_regs.clear();

    let raw = match app.current_backend().assemble(&app.code) {
        Ok(b) => b,
        Err(e) => {
            app.error_msg = Some(e);
            return;
        }
    };

    app.pc_to_line = raw.pc_to_line;
    app.line_to_pc = raw.line_to_pc;

    app.labels.clear();
    for (name, addr) in raw.labels {
        app.labels.insert(addr, name);
    }

    let emu_arch = app.current_backend().arch();
    let emu_mode = app.current_backend().default_mode();

    app.gui_peripherals.clear();
    let mut emu = HyperEmu::new(emu_arch, emu_mode).unwrap();

    for map in &app.active_maps {
        let perms = Perms::all();
        match map.dev_type {
            DeviceType::Ram => {
                emu.mem_map(
                    map.start,
                    map.size,
                    perms,
                    hyperemu::bus::BusDevice::Ram(Ram::new(map.size as usize)),
                );
            }
            DeviceType::Gpio => {
                let (dev, gui) = app.current_backend().create_gpio(map.name.clone());

                app.gui_peripherals.push(gui);
                emu.mem_map(map.start, map.size, perms, dev.into());
            }
            DeviceType::Uart => {
                let buf = Arc::new(Mutex::new(String::new()));
                app.gui_peripherals.push(Arc::new(Mutex::new(UartGui {
                    name: map.name.clone(),
                    buffer: Arc::clone(&buf),
                })));
                let writer = GuiUartWriter {
                    buffer: Arc::clone(&buf),
                };
                emu.mem_map(
                    map.start,
                    map.size,
                    perms,
                    hyperemu::bus::BusDevice::Dynamic(Box::new(Uart::new(writer))),
                );
            }
        }
    }

    if let Err(e) = emu.load_raw(&raw.bytes, 0) {
        app.error_msg = Some(format!("Loader Error: {:?}", e));
        return;
    }

    app.current_backend().setup_startup_state(&mut emu);

    let bps = Arc::clone(&app.breakpoints);
    let ignore_bp = Arc::clone(&app.ignore_next_bp);

    emu.hooks
        .add_code_hook(move |_cpu: &mut dyn Cpu, _bus: &mut MemoryBus, pc: u64| {
            let mut ignore = ignore_bp.lock().unwrap();
            if *ignore == Some(pc) {
                *ignore = None;
                return Ok(());
            }
            *ignore = None;

            if bps.lock().unwrap().contains(&pc) {
                return Err(hyperemu::EmuError::Breakpoint(0));
            }
            Ok(())
        });

    app.emu = Some(emu);
}

pub fn render_editor(ui: &mut egui::Ui, app: &mut EmuApp) {
    let is_mobile = ui.ctx().content_rect().width() < 800.0;

    ui.heading("Assembly Code");

    if is_mobile {
        ui.label(
            egui::RichText::new(
                "💡 Tip: Swipe the line numbers or drag the right scrollbar to scroll.",
            )
            .small()
            .color(egui::Color32::LIGHT_BLUE),
        );
    }

    let bps_arc = Arc::clone(&app.breakpoints);
    ui.horizontal(|ui| {
        ui.label("Add Breakpoint:");
        ui.add(egui::TextEdit::singleline(&mut app.breakpoint_input).desired_width(40.0));
        if ui.button("Add").clicked() {
            if let Ok(line) = app.breakpoint_input.parse::<usize>() {
                if let Some(&pc) = app.line_to_pc.get(&(line.saturating_sub(1))) {
                    bps_arc.lock().unwrap().insert(pc);
                }
            }
        }
        if ui.button("Clear All").clicked() {
            bps_arc.lock().unwrap().clear();
        }
    });
    ui.separator();

    let pc_reg = app.current_backend().pc_reg();
    let current_pc = app
        .emu
        .as_ref()
        .map_or(0, |e| e.reg_read(pc_reg).unwrap_or(0));
    let active_line = app.pc_to_line.get(&current_pc).copied();
    let font_id = egui::TextStyle::Monospace.resolve(ui.style());

    let prev_scroll_width = ui.spacing().scroll.bar_width;
    if is_mobile {
        ui.spacing_mut().scroll.bar_width = 24.0;
    }

    egui::ScrollArea::both()
        .id_salt("code_scroll")
        .scroll_bar_visibility(if is_mobile {
            egui::scroll_area::ScrollBarVisibility::AlwaysVisible
        } else {
            egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
        })
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;

                let gutter_width = 45.0;
                let (gutter_id, gutter_initial_rect) =
                    ui.allocate_space(egui::vec2(gutter_width, 0.0));

                let backend = app.current_backend();
                let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, _wrap: f32| {
                    let mut job = egui::text::LayoutJob::default();
                    job.wrap.max_width = f32::INFINITY;

                    let string = buffer.as_str();
                    let bps = bps_arc.lock().unwrap();
                    let parts: Vec<&str> = string.split('\n').collect();
                    let total_parts = parts.len();

                    for (i, line) in parts.iter().enumerate() {
                        let has_bp = app.line_to_pc.get(&i).map_or(false, |pc| bps.contains(pc));
                        let bg_color = if Some(i) == active_line {
                            egui::Color32::from_rgb(80, 80, 0)
                        } else if has_bp {
                            egui::Color32::from_rgb(80, 0, 0)
                        } else {
                            egui::Color32::TRANSPARENT
                        };

                        append_highlighted(
                            backend.as_ref(),
                            &mut job,
                            line,
                            bg_color,
                            font_id.clone(),
                        );

                        if i < total_parts - 1 {
                            job.append(
                                "\n",
                                0.0,
                                egui::text::TextFormat {
                                    font_id: font_id.clone(),
                                    color: egui::Color32::TRANSPARENT,
                                    background: bg_color,
                                    ..Default::default()
                                },
                            );
                        }
                    }
                    ui.fonts_mut(|f| f.layout_job(job))
                };

                let prev_extreme = ui.visuals().extreme_bg_color;
                ui.visuals_mut().extreme_bg_color = egui::Color32::TRANSPARENT;
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

                let output = egui::TextEdit::multiline(&mut app.code)
                    .code_editor()
                    .lock_focus(true)
                    .layouter(&mut layouter)
                    .margin(egui::vec2(8.0, 4.0))
                    .desired_width(f32::INFINITY)
                    .show(ui);

                ui.visuals_mut().extreme_bg_color = prev_extreme;

                let galley = output.galley;

                let gutter_rect = egui::Rect::from_min_size(
                    gutter_initial_rect.min,
                    egui::vec2(gutter_width, output.response.rect.height()),
                );

                let gutter_resp = ui.interact(gutter_rect, gutter_id, egui::Sense::click());
                let painter = ui.painter_at(gutter_rect);
                painter.rect_filled(gutter_rect, 0.0, egui::Color32::from_rgb(30, 30, 30));

                let mut bps = bps_arc.lock().unwrap();

                if gutter_resp.clicked() {
                    if let Some(pos) = gutter_resp.interact_pointer_pos() {
                        for (i, row) in galley.rows.iter().enumerate() {
                            let row_top = output.galley_pos.y + row.rect().min.y;
                            let row_bottom = output.galley_pos.y + row.rect().max.y;

                            if pos.y >= row_top && pos.y <= row_bottom {
                                if let Some(&pc) = app.line_to_pc.get(&i) {
                                    if bps.contains(&pc) {
                                        bps.remove(&pc);
                                    } else {
                                        bps.insert(pc);
                                    }
                                }
                                break;
                            }
                        }
                    }
                }

                for (i, row) in galley.rows.iter().enumerate() {
                    let text_y = output.galley_pos.y + row.rect().min.y;

                    painter.text(
                        egui::pos2(gutter_rect.right() - 5.0, text_y),
                        egui::Align2::RIGHT_TOP,
                        (i + 1).to_string(),
                        font_id.clone(),
                        egui::Color32::DARK_GRAY,
                    );

                    if app.line_to_pc.get(&i).map_or(false, |pc| bps.contains(pc)) {
                        let text_center_y = output.galley_pos.y + row.rect().center().y;
                        painter.circle_filled(
                            egui::pos2(gutter_rect.left() + 10.0, text_center_y),
                            4.0,
                            egui::Color32::RED,
                        );
                    }
                }
            });
        });

    if is_mobile {
        ui.spacing_mut().scroll.bar_width = prev_scroll_width;
    }
}

pub fn render_disassembly(ui: &mut egui::Ui, app: &mut EmuApp) {
    ui.heading("Live Disassembly");

    let pc_reg = app.current_backend().pc_reg();
    let backend = app.current_backend();

    if app.emu.is_some() {
        let pc = app
            .emu
            .as_ref()
            .expect("Can't be None")
            .reg_read(pc_reg)
            .unwrap_or(0);
        let start_addr = pc.saturating_sub(32);

        egui::ScrollArea::both()
            .id_salt("disasm_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let col_width = ui.available_width() / 4.0;

                egui::Grid::new("disasm_grid")
                    .striped(true)
                    .spacing([0.0, 2.0])
                    .min_col_width(col_width)
                    .show(ui, |ui| {
                        // HEADER ROW
                        let header_frame = egui::Frame::NONE.inner_margin(egui::vec2(10.0, 4.0));

                        header_frame.show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new("Address").strong());
                        });
                        header_frame.show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new("Bytes").strong());
                        });
                        header_frame.show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new("Disassembly").strong());
                        });
                        header_frame.show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new("Internal").strong());
                        });
                        ui.end_row();

                        // DATA ROWS
                        let mut current_addr = start_addr;

                        for _ in 0..64 {
                            // Label
                            if let Some(label_name) = app.labels.get(&current_addr) {
                                let label_frame =
                                    egui::Frame::NONE.inner_margin(egui::vec2(10.0, 4.0));

                                label_frame.show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                }); // Col 1 (Empty)
                                label_frame.show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                }); // Col 2 (Empty)
                                label_frame.show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        egui::RichText::new(format!("<{}>:", label_name))
                                            .monospace()
                                            .strong()
                                            .color(egui::Color32::from_rgb(220, 220, 170)), // Function yellow/tan
                                    );
                                });
                                label_frame.show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                }); // Col 4 (Empty)
                                ui.end_row();
                            }
                            // End Label

                            let is_pc = current_addr == pc;

                            let bg = if is_pc {
                                egui::Color32::from_rgb(60, 60, 60)
                            } else {
                                egui::Color32::TRANSPARENT
                            };

                            let cell_frame = egui::Frame::NONE
                                .fill(bg)
                                .inner_margin(egui::vec2(10.0, 4.0));

                            let backend = backend.as_ref();
                            let info = backend.disassemble(
                                current_addr,
                                app.emu.as_mut().expect("Can't be None"),
                            );

                            cell_frame.show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    egui::RichText::new(format!("0x{:08X}", current_addr))
                                        .monospace(),
                                );
                            });

                            cell_frame.show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    egui::RichText::new(&info.hex_bytes)
                                        .monospace()
                                        .color(egui::Color32::LIGHT_GRAY),
                                );
                            });

                            cell_frame.show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    egui::RichText::new(&info.disassembly)
                                        .monospace()
                                        .color(egui::Color32::LIGHT_GREEN),
                                );
                            });

                            cell_frame.show(ui, |ui| {
                                ui.set_width(ui.available_width());

                                let window_id = ui.id().with("ast_window").with(current_addr);
                                let mut show_ast =
                                    ui.data(|d| d.get_temp::<bool>(window_id).unwrap_or(false));

                                if ui.button("🔍 View AST").clicked() {
                                    show_ast = !show_ast;
                                    ui.data_mut(|d| d.insert_temp(window_id, show_ast));
                                }

                                if show_ast {
                                    let mut is_open = show_ast;

                                    egui::Window::new(format!("AST: 0x{:08X}", current_addr))
                                        .open(&mut is_open)
                                        .default_size([400.0, 300.0])
                                        .vscroll(true)
                                        .hscroll(true)
                                        .show(ui.ctx(), |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(&info.internal_enum)
                                                        .monospace()
                                                        .color(egui::Color32::LIGHT_BLUE),
                                                )
                                                .wrap_mode(egui::TextWrapMode::Extend),
                                            );
                                        });

                                    if !is_open {
                                        ui.data_mut(|d| d.insert_temp(window_id, false));
                                    }
                                }
                            });

                            ui.end_row();
                            current_addr += info.byte_size as u64;
                        }
                    });
            });
    } else {
        ui.label("Compile and load to view disassembly.");
    }
}

pub fn render_memory_map(ui: &mut egui::Ui, app: &mut EmuApp) {
    egui::Grid::new("mem_input_grid").show(ui, |ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut app.map_input_name);
        ui.end_row();
        ui.label("Addr:");
        ui.text_edit_singleline(&mut app.map_input_addr);
        ui.end_row();
        ui.label("Size:");
        ui.text_edit_singleline(&mut app.map_input_size);
        ui.end_row();
        ui.label("Type:");
        egui::ComboBox::from_id_salt("mem_type")
            .selected_text(match app.map_input_type {
                DeviceType::Ram => "RAM",
                DeviceType::Gpio => "GPIO",
                DeviceType::Uart => "UART",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut app.map_input_type, DeviceType::Ram, "RAM");
                ui.selectable_value(&mut app.map_input_type, DeviceType::Gpio, "GPIO");
                ui.selectable_value(&mut app.map_input_type, DeviceType::Uart, "UART");
            });
        ui.end_row();
    });

    if ui.button("Add Peripheral").clicked() {
        let start =
            u64::from_str_radix(app.map_input_addr.trim_start_matches("0x"), 16).unwrap_or(0);
        let size =
            u64::from_str_radix(app.map_input_size.trim_start_matches("0x"), 16).unwrap_or(0);
        app.active_maps.push(MemMapRecord {
            name: app.map_input_name.clone(),
            start,
            size,
            dev_type: app.map_input_type.clone(),
        });
    }

    ui.separator();
    ui.heading("Active Regions");
    egui::ScrollArea::vertical()
        .id_salt("map_scroll")
        .show(ui, |ui| {
            let mut to_remove = None;
            for (i, map) in app.active_maps.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "0x{:08X} ({}B) - {} [{:?}]",
                        map.start, map.size, map.name, map.dev_type
                    ));

                    if map.name == "Code" || map.name == "Stack" {
                        ui.label(
                            egui::RichText::new("(Protected)").color(egui::Color32::DARK_GRAY),
                        );
                    } else {
                        // Create a unique UI state ID based on the peripheral's starting address
                        // (We use `map.start` instead of `i` so states don't shift when an item is deleted!)
                        let confirm_id = ui.id().with("confirm_remove").with(map.start);

                        // Check egui's temporary state to see if this specific item is confirming
                        let is_confirming =
                            ui.data(|d| d.get_temp::<bool>(confirm_id).unwrap_or(false));

                        if is_confirming {
                            if ui
                                .button(egui::RichText::new("⚠ Confirm").color(egui::Color32::RED))
                                .clicked()
                            {
                                to_remove = Some(i);
                                ui.data_mut(|d| d.insert_temp(confirm_id, false));
                            }
                            if ui.button("Cancel").clicked() {
                                ui.data_mut(|d| d.insert_temp(confirm_id, false));
                            }
                        } else {
                            if ui.button("Remove").clicked() {
                                // Set this specific peripheral to the "confirming" state
                                ui.data_mut(|d| d.insert_temp(confirm_id, true));
                            }
                        }
                    }
                });
            }
            if let Some(idx) = to_remove {
                app.active_maps.remove(idx);
            }
        });
}

pub fn render_memory_view(ui: &mut egui::Ui, app: &mut EmuApp) {
    ui.horizontal(|ui| {
        ui.label("Base Address: ");
        ui.text_edit_singleline(&mut app.memory_base_input);
        if ui.button("Go").clicked() {
            let clean = app.memory_base_input.trim_start_matches("0x").trim();
            if let Ok(addr) = u64::from_str_radix(clean, 16) {
                app.memory_base_addr = addr & 0xFFFFFFFFFFFFFFF0;
            }
        }
    });
    ui.separator();

    if let Some(emu) = &mut app.emu {
        egui::ScrollArea::vertical()
            .id_salt("mem_scroll")
            .show(ui, |ui| {
                egui::Grid::new("mem_grid")
                    .striped(true)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Address").strong());
                        for i in 0..16 {
                            ui.label(egui::RichText::new(format!("{:02X}", i)).strong());
                        }
                        ui.label(egui::RichText::new("ASCII").strong());
                        ui.end_row();

                        for row in 0..128 {
                            let addr = app.memory_base_addr.wrapping_add(row * 16);
                            ui.label(
                                egui::RichText::new(format!("0x{:016X}", addr))
                                    .monospace()
                                    .color(egui::Color32::DARK_GRAY),
                            );

                            let mut chunk = [0u8; 16];
                            let mapped = emu.bus.read_bytes(addr, &mut chunk).is_ok();
                            let mut ascii = String::new();

                            for b in chunk {
                                if mapped {
                                    ui.label(egui::RichText::new(format!("{:02X}", b)).monospace());
                                    ascii.push(if b >= 32 && b <= 126 { b as char } else { '.' });
                                } else {
                                    ui.label(
                                        egui::RichText::new("??")
                                            .monospace()
                                            .color(egui::Color32::DARK_GRAY),
                                    );
                                    ascii.push('.');
                                }
                            }
                            ui.label(
                                egui::RichText::new(ascii)
                                    .monospace()
                                    .color(egui::Color32::LIGHT_GREEN),
                            );
                            ui.end_row();
                        }
                    });
            });
    } else {
        ui.label("Emulator not loaded.");
    }
}

pub fn render_cpu(ui: &mut egui::Ui, app: &mut EmuApp) {
    ui.heading("Registers");
    if let Some(emu) = &app.emu {
        app.current_backend()
            .render_registers(ui, emu, &app.prev_regs, &app.labels);
    } else {
        ui.label("Emulator not loaded.");
    }
}

pub fn render_stack(ui: &mut egui::Ui, app: &mut EmuApp) {
    ui.heading("Live Stack (Top 16)");

    let backend = app.current_backend();
    let sp_reg = backend.sp_reg();
    let word_size = backend.word_size() as u64;

    if let Some(emu) = &mut app.emu {
        let sp = emu.reg_read(sp_reg).unwrap_or(0);
        // 4 Columns = 3 gaps * 15.0px = 45.0px + 10px buffer = 55.0px
        let col_width = (ui.available_width() - 55.0) / 4.0;

        egui::Grid::new("stack_grid")
            .num_columns(4) // 1. Add 4th column
            .striped(true)
            .min_col_width(col_width)
            .spacing([15.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Address").strong());
                ui.label(egui::RichText::new("Value").strong());
                ui.label(egui::RichText::new("Label").strong()); // New Header
                ui.label("");
                ui.end_row();

                for i in 0..16 {
                    let addr = sp.wrapping_add(i * word_size);

                    // 1. Address Column
                    ui.label(
                        egui::RichText::new(format!("0x{:08X}", addr))
                            .monospace()
                            .color(egui::Color32::DARK_GRAY),
                    );

                    // 2 & 3. Value & Label Columns
                    match emu.bus.read_32(addr) {
                        Ok(val) => {
                            let prev = app.prev_stack.get(&addr).copied().unwrap_or(val);
                            let color = if val != prev {
                                egui::Color32::YELLOW
                            } else {
                                ui.visuals().text_color()
                            };

                            ui.colored_label(
                                color,
                                egui::RichText::new(format!("0x{:08X}", val)).monospace(),
                            );

                            // Put label in its own column!
                            if let Some(lbl) = app.labels.get(&(val as u64)) {
                                let text = format!("<{}>", lbl);
                                let resp = ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&text)
                                            .color(egui::Color32::from_rgb(220, 220, 170)),
                                    )
                                    .truncate(), // Crucial: Allows the column to shrink!
                                );
                                resp.on_hover_text(text);
                            } else {
                                ui.allocate_space(egui::Vec2::ZERO);
                            }
                        }
                        Err(_) => {
                            ui.label(egui::RichText::new("Unmapped").color(egui::Color32::RED));
                            ui.allocate_space(egui::Vec2::ZERO);
                        }
                    }

                    // 4. Indicator Column
                    if i == 0 {
                        ui.label(
                            egui::RichText::new("← SP")
                                .color(egui::Color32::LIGHT_BLUE)
                                .strong(),
                        );
                    } else {
                        ui.allocate_space(egui::Vec2::ZERO);
                    }

                    ui.end_row();
                }
            });
    } else {
        ui.label(
            egui::RichText::new("Emulator not loaded.")
                .italics()
                .color(egui::Color32::DARK_GRAY),
        );
    }
}

pub fn render_dynamic_gpios(ui: &mut egui::Ui, app: &mut EmuApp) {
    ui.heading("Hardware Components");
    let mut found = false;

    for p in &mut app.gui_peripherals {
        let mut p_locked = p.lock().unwrap();
        if p_locked.category() == PeripheralCategory::Hardware {
            p_locked.render(ui);
            found = true;
        }
    }

    if !found {
        ui.label(
            egui::RichText::new("No Hardware components mapped.")
                .italics()
                .color(egui::Color32::DARK_GRAY),
        );
    }
}

pub fn render_consoles(ui: &mut egui::Ui, app: &mut EmuApp) {
    let mut found = false;

    for p in &mut app.gui_peripherals {
        let mut p_locked = p.lock().unwrap();
        if p_locked.category() == PeripheralCategory::Console {
            p_locked.render(ui);
            found = true;
        }
    }

    if !found {
        ui.label(
            egui::RichText::new("No Consoles mapped.")
                .italics()
                .color(egui::Color32::DARK_GRAY),
        );
    }
}
