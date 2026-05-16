use super::ArchBackend;
use crate::{
    backend::{AssembleResult, DisassemblyInfo},
    ui::peripherals::{GuiPeripheral, PeripheralCategory},
};
use armv7_disassembler::disassembler::{Decoder, Endian};
use armv7_encoder::assembler::Encoder;
use eframe::egui;
use hyperemu::{
    Arch, CpuMode, HyperEmu,
    device::{self, Device, stm32_gpio::Stm32Gpio},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub struct Armv7Backend;

const STARTER_CODE: &str = r#".text
.global _start
_start:
    @ UART base address
    ldr r7, =0x10000000

    @ Pointer to string
    ldr r1, =message
print_loop: ldrb r2, [r1]
    @ null terminator?
    cmp r2, #0
    beq finished_printing

    @ write byte to UART DATA register
    strb r2, [r7]
    add r1, r1, #1
    b print_loop

finished_printing:
    @ Configure GPIO (0x40000000)
    mov     r0, #0x40000000

    @ Set PA5 as output (MODER bit 10 = 1)
    mov     r1, #0x400
    str     r1, [r0]

loop:
    @ 1. Read the IDR (Input Data Register) at offset 0x10 -> triggers registers[4]
    ldr     r2, [r0, #0x10]
    
    @ 2. Mask out everything except bit 0 (our UI button)
    and     r2, r2, #1
    
    @ 3. Is the button pressed?
    cmp     r2, #1
    beq     turn_led_on

turn_led_off:
    mov     r1, #0
    str     r1, [r0, #0x14]  @ Write to ODR -> registers[5]
    b       loop

turn_led_on:
    @ Turn LED ON (ODR bit 5 = 1 -> 0x20)
    mov     r1, #0x20
    str     r1,[r0, #0x14]
    b       loop

    bkpt #0

.data
message: .ascii "Hello, World!\n"
_message_null: .byte 0
    .align 4
message_len: .word _message_null - message
"#;

impl ArchBackend for Armv7Backend {
    fn arch(&self) -> Arch {
        Arch::Armv7
    }

    fn id(&self) -> &'static str {
        "armv7"
    }

    fn name(&self) -> &'static str {
        "ARMv7 (32-bit)"
    }

    fn default_code(&self) -> &'static str {
        STARTER_CODE
    }

    fn default_mode(&self) -> CpuMode {
        CpuMode::MODE_32
    }

    fn pc_reg(&self) -> usize {
        15
    }

    fn sp_reg(&self) -> usize {
        13
    }

    fn num_registers(&self) -> usize {
        17 // R0 through R15 (16) + CPSR (1)
    }

    fn word_size(&self) -> usize {
        4 // 32-bit Architecture
    }

    fn setup_startup_state(&self, emu: &mut HyperEmu) {
        emu.reg_write(self.sp_reg(), 0xB000).unwrap(); // Standard SP
        emu.reg_write(self.pc_reg(), 0).unwrap(); // Standard PC
    }

    fn assemble(&self, code: &str) -> Result<AssembleResult, String> {
        match Encoder::new().start_address(0).assemble(code) {
            Ok(res) => {
                let mut bytes = Vec::new();
                for w in res.bytes {
                    bytes.extend_from_slice(&w.to_le_bytes());
                }

                // ARM instructions are fixed 4-bytes, so mapping is straightforward here
                let mut pc_to_line = HashMap::new();
                let mut line_to_pc = HashMap::new();
                let mut current_pc = 0u64;

                for (i, line) in code.split('\n').enumerate() {
                    let trimmed = line.trim();
                    if trimmed.is_empty()
                        || trimmed.starts_with('@')
                        || trimmed.ends_with(':')
                        || trimmed.starts_with('.')
                    {
                        continue;
                    }
                    pc_to_line.insert(current_pc, i);
                    line_to_pc.insert(i, current_pc);
                    current_pc += 4;
                }

                Ok(AssembleResult {
                    bytes,
                    entry_point: res.entry_point,
                    labels: res.labels,
                    instruction_count: res.instruction_count,
                    line_to_pc,
                    pc_to_line,
                })
            }
            Err(e) => Err(format!("{:?}", e)),
        }
    }

    fn disassemble(&self, addr: u64, emu: &mut HyperEmu) -> DisassemblyInfo {
        // 1. Read CPSR (Register 16) to determine current Endianness (Bit 9)
        let cpsr = emu.reg_read(16).unwrap_or(0);
        let is_big_endian = (cpsr & (1 << 9)) != 0;
        let endian = if is_big_endian {
            Endian::Big
        } else {
            Endian::Little
        };

        // 2. Read exact physical bytes from the memory bus
        let mut bytes = [0u8; 4];
        if emu.bus.read_bytes(addr, &mut bytes).is_ok() {
            // Format bytes in strict physical memory order
            let bytes_str = format!(
                "{:02X} {:02X} {:02X} {:02X}",
                bytes[0], bytes[1], bytes[2], bytes[3]
            );

            // 3. Reconstruct the 32-bit word correctly depending on Endianness
            let raw = if is_big_endian {
                u32::from_be_bytes(bytes)
            } else {
                u32::from_le_bytes(bytes)
            };

            let dis_str = Decoder::new()
                .start_address(addr as _)
                .endian(endian)
                .disassemble(&bytes)
                .unwrap_or_default()
                .join(" ");

            let enum_str = format!("{:?}", hyperemu::arch::armv7::decode::decode_arm(raw));

            DisassemblyInfo::new(bytes_str, dis_str, enum_str, 4)
        } else {
            DisassemblyInfo::new("?? ?? ?? ??".into(), "Invalid Memory".into(), "".into(), 4)
        }
    }

    fn create_gpio(
        &self,
        name: String,
    ) -> (
        Arc<Mutex<dyn device::Device + Send>>,
        Arc<Mutex<dyn GuiPeripheral>>,
    ) {
        let dev = Arc::new(Mutex::new(Stm32Gpio::new()));
        let gui = Arc::new(Mutex::new(Stm32GpioGui {
            name,
            device: Arc::clone(&dev),
        }));
        (dev, gui)
    }

    fn render_registers(
        &self,
        ui: &mut egui::Ui,
        emu: &HyperEmu,
        prev_regs: &HashMap<usize, u64>,
        labels: &HashMap<u64, String>,
    ) {
        let col_width = (ui.available_width() - 30.0) / 3.0;

        egui::Grid::new("reg_grid")
            .num_columns(3) // 3 Columns
            .striped(true)
            .min_col_width(col_width)
            .show(ui, |ui| {
                for r in 0..16 {
                    let val = emu.reg_read(r).unwrap_or(0);
                    let prev = prev_regs.get(&r).copied().unwrap_or(val);
                    let color = if val != prev {
                        egui::Color32::YELLOW
                    } else {
                        ui.visuals().text_color()
                    };

                    // 1. Name Column
                    let name = match r {
                        13 => Some("SP"),
                        14 => Some("LR"),
                        15 => Some("PC"),
                        _ => None,
                    };
                    if let Some(name) = name {
                        ui.label(name);
                    } else {
                        ui.label(format!("R{}", r));
                    }

                    // 2. Value Column
                    ui.colored_label(
                        color,
                        egui::RichText::new(format!("0x{:08X}", val)).monospace(),
                    );

                    // 3. Label Column
                    if let Some(lbl) = labels.get(&val) {
                        ui.label(
                            egui::RichText::new(format!("<{}>", lbl))
                                .color(egui::Color32::from_rgb(220, 220, 170)),
                        );
                    } else {
                        ui.allocate_space(egui::Vec2::ZERO);
                    }

                    ui.end_row();
                }
            });

        ui.separator();
        ui.heading("CPSR Flags");
        let cpsr = emu.reg_read(16).unwrap_or(0);
        let n = (cpsr >> 31) & 1;
        let z = (cpsr >> 30) & 1;
        let c = (cpsr >> 29) & 1;
        let v = (cpsr >> 28) & 1;
        let i = (cpsr >> 7) & 1;
        let f = (cpsr >> 6) & 1;
        let t = (cpsr >> 5) & 1;
        let e = (cpsr >> 9) & 1;

        egui::Grid::new("cpsr_grid")
            .num_columns(2)
            .striped(true)
            .min_col_width(col_width)
            .show(ui, |ui| {
                let red = egui::Color32::RED;
                let gray = egui::Color32::DARK_GRAY;
                ui.label("N (Negative):");
                ui.colored_label(if n == 1 { red } else { gray }, n.to_string());
                ui.end_row();
                ui.label("Z (Zero):");
                ui.colored_label(if z == 1 { red } else { gray }, z.to_string());
                ui.end_row();
                ui.label("C (Carry):");
                ui.colored_label(if c == 1 { red } else { gray }, c.to_string());
                ui.end_row();
                ui.label("V (Overflow):");
                ui.colored_label(if v == 1 { red } else { gray }, v.to_string());
                ui.end_row();
                ui.label("I (IRQ Mask):");
                ui.colored_label(if i == 1 { red } else { gray }, i.to_string());
                ui.end_row();
                ui.label("F (FIQ Mask):");
                ui.colored_label(if f == 1 { red } else { gray }, f.to_string());
                ui.end_row();
                ui.label("T (Thumb):");
                ui.colored_label(if t == 1 { red } else { gray }, t.to_string());
                ui.end_row();
                ui.label("E (Endian):");
                ui.colored_label(
                    if e == 1 { red } else { gray },
                    if e == 1 { "Big" } else { "Little" },
                );
                ui.end_row();
            });
    }

    fn is_instruction(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        match lower.as_str() {
            "add" | "sub" | "mov" | "ldr" | "str" | "b" | "beq" | "bne" | "cmp" | "subs"
            | "ldrb" | "strb" | "lsl" | "lsr" | "asr" | "and" | "orr" | "eor" | "push" | "pop"
            | "svc" | "bl" | "bx" | "blx" | "mul" | "mla" | "nop" | "bkpt" => true,
            _ => false,
        }
    }

    fn is_register(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        match lower.as_str() {
            "r0" | "r1" | "r2" | "r3" | "r4" | "r5" | "r6" | "r7" | "r8" | "r9" | "r10" | "r11"
            | "r12" | "r13" | "sp" | "r14" | "lr" | "r15" | "pc" => true,
            _ => false,
        }
    }
}

// Custom GUI frontend wrapper mapping rendering bounds to Stm32Gpio explicitly.
pub struct Stm32GpioGui {
    pub name: String,
    pub device: Arc<Mutex<Stm32Gpio>>,
}

impl GuiPeripheral for Stm32GpioGui {
    fn name(&self) -> &str {
        &self.name
    }

    fn category(&self) -> PeripheralCategory {
        PeripheralCategory::Hardware
    }

    fn render(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(&self.name).strong());

                let mut dev = self.device.lock().unwrap();

                ui.horizontal(|ui| {
                    // Raw Read from IDR (Offset 0x10) using your trait implementation
                    let current_idr = dev.read_32(0x10).unwrap_or(0);
                    let mut is_pressed = (current_idr & 1) != 0;

                    // The Checkbox
                    if ui
                        .checkbox(&mut is_pressed, "Toggle Pin 0 (Input)")
                        .changed()
                    {
                        // Raw Write to IDR (Offset 0x10)
                        let new_idr = if is_pressed {
                            current_idr | 1
                        } else {
                            current_idr & !1
                        };
                        let _ = dev.write_32(0x10, new_idr);
                    }

                    ui.add_space(20.0);

                    let is_on = dev.is_led_on();
                    let color = if is_on {
                        egui::Color32::from_rgb(50, 255, 50)
                    } else {
                        egui::Color32::from_rgb(40, 40, 40)
                    };

                    ui.label("LED:");
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 8.0, color);
                    ui.painter().circle_stroke(
                        rect.center(),
                        8.0,
                        egui::Stroke::new(1.0, egui::Color32::DARK_GRAY),
                    );
                });

                ui.separator();

                // Live Memory Diagnostics
                ui.label(
                    egui::RichText::new("Hardware Register Debug:")
                        .small()
                        .color(egui::Color32::DARK_GRAY),
                );
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "MODER: 0x{:08X}",
                            dev.read_32(0x00).unwrap_or(0)
                        ))
                        .monospace()
                        .small(),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "IDR:   0x{:08X}",
                            dev.read_32(0x10).unwrap_or(0)
                        ))
                        .monospace()
                        .small(),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "ODR:   0x{:08X}",
                            dev.read_32(0x14).unwrap_or(0)
                        ))
                        .monospace()
                        .small(),
                    );
                });
            });
        });
    }
}
