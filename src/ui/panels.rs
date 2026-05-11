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
    app.build_pc_map();

    let raw = match app.current_backend().assemble(&app.code) {
        Ok(b) => b,
        Err(e) => {
            app.error_msg = Some(e);
            return;
        }
    };

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

    if let Err(e) = emu.load_raw(&raw, 0) {
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

    // MOBILE CODE:
    // Add a helpful hint for mobile users
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
        ui.label("Add Breakpoint (Line):");
        ui.add(egui::TextEdit::singleline(&mut app.breakpoint_input).desired_width(50.0));
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
    let row_height = ui.fonts_mut(|f| f.row_height(&font_id));

    // TextEdit has a default internal Y margin of 4.0. We use this to align the gutter.
    let text_margin_y = 4.0;

    let prev_scroll_width = ui.spacing().scroll.bar_width;
    if is_mobile {
        ui.spacing_mut().scroll.bar_width = 10.0; // Make it massive and easy to grab!
    }

    egui::ScrollArea::both()
        .id_salt("code_scroll")
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;

                let num_lines = app.code.split('\n').count().max(1);

                // The Gutter
                let (gutter_rect, gutter_resp) = ui.allocate_exact_size(
                    egui::vec2(
                        45.0,
                        (num_lines as f32 * row_height) + (text_margin_y * 2.0),
                    ),
                    egui::Sense::click(),
                );

                let mut bps = bps_arc.lock().unwrap();

                if gutter_resp.clicked() {
                    if let Some(pos) = gutter_resp.interact_pointer_pos() {
                        // Calculate exact line index factoring in the margin
                        let y_offset = pos.y - gutter_rect.top() - text_margin_y;
                        if y_offset >= 0.0 {
                            let mut line_idx = (y_offset / row_height) as usize;
                            line_idx = line_idx.min(num_lines.saturating_sub(1));

                            if let Some(&pc) = app.line_to_pc.get(&line_idx) {
                                if bps.contains(&pc) {
                                    bps.remove(&pc);
                                } else {
                                    bps.insert(pc);
                                }
                            }
                        }
                    }
                }

                if ui.is_rect_visible(gutter_rect) {
                    ui.painter()
                        .rect_filled(gutter_rect, 0.0, egui::Color32::from_rgb(30, 30, 30));
                    for i in 0..num_lines {
                        let y = gutter_rect.top() + text_margin_y + (i as f32 * row_height);
                        ui.painter().text(
                            egui::pos2(gutter_rect.right() - 5.0, y),
                            egui::Align2::RIGHT_TOP,
                            (i + 1).to_string(),
                            font_id.clone(),
                            egui::Color32::DARK_GRAY,
                        );

                        if app.line_to_pc.get(&i).map_or(false, |pc| bps.contains(pc)) {
                            ui.painter().circle_filled(
                                egui::pos2(gutter_rect.left() + 10.0, y + (row_height / 2.0)),
                                4.0,
                                egui::Color32::RED,
                            );
                        }
                    }
                }
                drop(bps);

                // The Text Editor
                let backend = app.current_backend();
                let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, _wrap: f32| {
                    let mut job = egui::text::LayoutJob::default();
                    let string = buffer.as_str();
                    let bps = bps_arc.lock().unwrap();

                    for (i, line) in string.split('\n').enumerate() {
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
                    ui.fonts_mut(|f| f.layout_job(job))
                };

                let prev_extreme = ui.visuals().extreme_bg_color;
                ui.visuals_mut().extreme_bg_color = egui::Color32::TRANSPARENT;

                ui.add(
                    egui::TextEdit::multiline(&mut app.code)
                        .code_editor()
                        .lock_focus(true)
                        .layouter(&mut layouter)
                        .margin(egui::vec2(8.0, text_margin_y)) // Match gutter margin exactly
                        .desired_width(ui.available_width()),
                );

                ui.visuals_mut().extreme_bg_color = prev_extreme;
            });
        });

    // MOBILE CODE:
    // Restore the scrollbar width for the rest of the application
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
            .show(ui, |ui| {
                egui::Grid::new("disasm_grid")
                    .striped(true)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Address").strong());
                        ui.label(egui::RichText::new("Bytes").strong());
                        ui.label(egui::RichText::new("Disassembly").strong());
                        ui.label(egui::RichText::new("Internal").strong());
                        ui.end_row();

                        let mut current_addr = start_addr;

                        for _ in 0..64 {
                            let is_pc = current_addr == pc;
                            let bg = if is_pc {
                                egui::Color32::from_rgb(60, 60, 60)
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            ui.painter()
                                .rect_filled(ui.available_rect_before_wrap(), 0.0, bg);

                            let backend = backend.as_ref();
                            let info = backend.disassemble(
                                current_addr,
                                app.emu.as_mut().expect("Can't be None"),
                            );

                            ui.label(
                                egui::RichText::new(format!("0x{:08X}", current_addr)).monospace(),
                            );
                            ui.label(
                                egui::RichText::new(&info.hex_bytes)
                                    .monospace()
                                    .color(egui::Color32::LIGHT_GRAY),
                            );
                            ui.label(
                                egui::RichText::new(&info.disassembly)
                                    .monospace()
                                    .color(egui::Color32::LIGHT_GREEN),
                            );

                            // Generate a unique ID for this address's window state
                            let window_id = ui.id().with("ast_window").with(current_addr);
                            let mut show_ast =
                                ui.data(|d| d.get_temp::<bool>(window_id).unwrap_or(false));

                            if ui.button("🔍 View AST").clicked() {
                                show_ast = !show_ast;
                                ui.data_mut(|d| d.insert_temp(window_id, show_ast));
                            }

                            // If the user clicked the button, spawn a floating window!
                            if show_ast {
                                let mut is_open = show_ast;

                                egui::Window::new(format!("AST: 0x{:08X}", current_addr))
                                    .open(&mut is_open) // Adds an "X" button to close it
                                    .default_size([400.0, 300.0])
                                    .vscroll(true)
                                    .hscroll(true) // Prevents the text from ever squishing!
                                    .show(ui.ctx(), |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(&info.internal_enum)
                                                    .monospace()
                                                    .color(egui::Color32::LIGHT_BLUE),
                                            )
                                            .wrap_mode(egui::TextWrapMode::Extend), // Force text to stretch naturally
                                        );
                                    });

                                // If the user clicked the 'X' to close the window, update our state
                                if !is_open {
                                    ui.data_mut(|d| d.insert_temp(window_id, false));
                                }
                            }

                            ui.end_row();
                            current_addr += info.byte_size;
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
                                ui.data_mut(|d| d.insert_temp(confirm_id, false)); // Reset state
                            }
                            if ui.button("Cancel").clicked() {
                                ui.data_mut(|d| d.insert_temp(confirm_id, false)); // Cancel state
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
            .render_registers(ui, emu, &app.prev_regs);
    } else {
        ui.label("Emulator not loaded.");
    }
}

pub fn render_stack(ui: &mut egui::Ui, app: &mut EmuApp) {
    ui.heading("Live Stack (Top 16)");

    let sp_reg = app.current_backend().sp_reg();

    if let Some(emu) = &mut app.emu {
        let sp = emu.reg_read(sp_reg).unwrap_or(0);

        egui::Grid::new("stack_grid")
            .num_columns(3) // Address | Value | Indicator
            .striped(true)
            .spacing([15.0, 4.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Address").strong());
                ui.label(egui::RichText::new("Value").strong());
                ui.label(""); // Empty header for the arrow indicator
                ui.end_row();

                for i in 0..16 {
                    let addr = sp + (i * 4) as u64;

                    // Address Column
                    ui.label(
                        egui::RichText::new(format!("0x{:08X}", addr))
                            .monospace()
                            .color(egui::Color32::DARK_GRAY),
                    );

                    // Value Column
                    match emu.bus.read_32(addr) {
                        Ok(val) => {
                            let prev = app.prev_stack.get(&addr).copied().unwrap_or(val);
                            let color = if val != prev {
                                egui::Color32::YELLOW // Highlight changes!
                            } else {
                                ui.visuals().text_color()
                            };

                            ui.colored_label(
                                color,
                                egui::RichText::new(format!("0x{:08X}", val)).monospace(),
                            );
                        }
                        Err(_) => {
                            ui.label(egui::RichText::new("Unmapped").color(egui::Color32::RED));
                        }
                    }

                    // Indicator Column
                    if i == 0 {
                        ui.label(
                            egui::RichText::new("← SP")
                                .color(egui::Color32::LIGHT_BLUE)
                                .strong(),
                        );
                    } else {
                        ui.label("");
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
