use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui;
use hyperemu::{Arch, CpuMode, HyperEmu, arch::x86::decode::X86Decoder, device::Device};
use x86_translator::{assembler::Assembler, disassembler::Disassembler};

use crate::backend::{ArchBackend, AssembleResult, DisassemblyInfo};
use crate::ui::peripherals::GuiPeripheral;

pub struct X86Backend;

const STARTER_CODE: &str = r#".text
.global _start
_start:
    ; UART base address (the console to write to)
    mov ebx, 0x10000000

    cld
    ; Pointer to string
    lea esi, [message]
    ; Load the length into the register
	mov ecx, dword ptr [message_len]
print_loop:
    lodsb
    mov byte ptr [ebx], al
    loop print_loop

finished_printing:
    ; GPIO Base Address
    mov ebx, 0x40000000

main_loop:
    ; Read the BUTTONS register (Offset 0x04)
    mov eax, dword ptr [ebx + 4]

    ; Mask out everything except bit 0 (our Checkbox)
    and al, 1

    ; Is the button pressed?
    ; Since we mask everything out except 1 (eax can only be 1 or 0)
    ; The only '1' we can have is the lower byte
    ; (which is al. al, ah are the low and high bytes of ax, which is the lower two bytes of eax)
    test al, al
    ; If zero flag is not set, jump to turn_led_on,
    ; i.e. jump to turn_led_on if it is not zero.
    jne turn_led_on

turn_led_off:
    ; Write 0 to the LEDS register (Offset 0x00)
    mov dword ptr [ebx], 0
    jmp main_loop

turn_led_on:
    ; Write 1 to the LEDS register (Offset 0x00)
    mov dword ptr [ebx], 1
    jmp main_loop

done:
    ; Done, loop
    jmp done

message:
	.ascii "Hello, World!\n"
_message_null: .byte 0
    .align 4
message_len: .long _message_null - message
"#;

impl ArchBackend for X86Backend {
    fn arch(&self) -> Arch {
        Arch::X86
    }

    fn id(&self) -> &'static str {
        "x86"
    }

    fn name(&self) -> &'static str {
        "x86 (32-bit i386)"
    }

    fn default_code(&self) -> &'static str {
        STARTER_CODE
    }

    fn default_mode(&self) -> CpuMode {
        CpuMode::MODE_32
    }

    fn pc_reg(&self) -> usize {
        8 // REG_EIP in our emulator
    }

    fn sp_reg(&self) -> usize {
        4 // REG_ESP in our emulator
    }

    fn num_registers(&self) -> usize {
        16 // 8 GPRs + EIP + EFLAGS + 6 Segments
    }

    fn word_size(&self) -> usize {
        4 // 32-bit Architecture
    }

    fn setup_startup_state(&self, emu: &mut HyperEmu) {
        emu.reg_write(self.sp_reg(), 0xB000).unwrap();
        emu.reg_write(self.pc_reg(), 0).unwrap();
    }

    fn assemble(&self, code: &str) -> Result<AssembleResult, String> {
        // We pass the entire code block to the assembler exactly once.
        let res = Assembler::new()
            .bitness(32)
            .assemble(code)
            .map_err(|e| format!("{}", e))?; // Changed from {:?} to {} to render the Display message cleanly

        Ok(AssembleResult {
            bytes: res.bytes,
            entry_point: res.entry_point,
            labels: res.labels,
            instruction_count: res.instruction_count,
            line_to_pc: res
                .line_to_ip
                .into_iter()
                .map(|(line, ip)| (line - 1, ip))
                .collect(),
            pc_to_line: res
                .ip_to_line
                .into_iter()
                .map(|(ip, line)| (ip, line - 1))
                .collect(),
        })
    }

    fn disassemble(&self, addr: u64, emu: &mut HyperEmu) -> DisassemblyInfo {
        let mut bytes = [0u8; 15]; // 15 bytes is the max instruction length for x86

        if emu.bus.read_bytes(addr, &mut bytes).is_ok() {
            let mut emu_decoder = X86Decoder::new(&bytes);
            let internal_ast = emu_decoder.decode_instr();

            let size = {
                let size = emu_decoder.consumed();
                if size == 0 { 1 } else { size }
            };

            let instr_bytes = &bytes[0..size as usize];

            let bytes_str = instr_bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");

            let internal_enum = format!("{:#?}", internal_ast);

            let dis_str = match Disassembler::new()
                .bitness(32)
                .start_address(addr)
                .disassemble(instr_bytes)
            {
                Ok(results) if !results.is_empty() => results[0].clone(),
                _ => "???".to_string(),
            };

            DisassemblyInfo::new(bytes_str, dis_str, internal_enum, size)
        } else {
            DisassemblyInfo::new("??".into(), "Invalid Memory".into(), "".into(), 1)
        }
    }

    fn create_gpio(
        &self,
        name: String,
    ) -> (Arc<Mutex<dyn Device + Send>>, Arc<Mutex<dyn GuiPeripheral>>) {
        let dev = Arc::new(Mutex::new(device::X86Gpio::new()));
        let gui = Arc::new(Mutex::new(X86GpioGui {
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
        let gap = ui.spacing().item_spacing.x;
        // 3 Columns = 2 gaps + 20px buffer
        let reg_col_width = (ui.available_width() - (gap * 2.0) - 20.0) / 3.0;

        egui::Grid::new("x86_reg_grid")
            .num_columns(3) // 3 Columns
            .striped(true)
            .min_col_width(reg_col_width)
            .show(ui, |ui| {
                let names = [
                    "EAX", "ECX", "EDX", "EBX", "ESP", "EBP", "ESI", "EDI", "EIP", "EFLAGS",
                ];
                for (i, name) in names.iter().enumerate() {
                    let val = emu.reg_read(i).unwrap_or(0);
                    let prev = prev_regs.get(&i).copied().unwrap_or(val);
                    let color = if val != prev {
                        egui::Color32::YELLOW
                    } else {
                        ui.visuals().text_color()
                    };

                    // 1. Name Column
                    ui.label(*name);

                    // 2. Value Column
                    ui.colored_label(
                        color,
                        egui::RichText::new(format!("0x{:08X}", val)).monospace(),
                    );

                    // 3. Label Column
                    if let Some(lbl) = labels.get(&val) {
                        let text = format!("<{}>", lbl);
                        let resp = ui.add(
                            egui::Label::new(
                                egui::RichText::new(&text)
                                    .color(egui::Color32::from_rgb(220, 220, 170)),
                            )
                            .truncate(),
                        );
                        resp.on_hover_text(text);
                    } else {
                        ui.allocate_space(egui::Vec2::ZERO);
                    }

                    ui.end_row();
                }
            });

        ui.separator();
        ui.heading("EFLAGS");

        // 2 Columns = 1 gap + 10px buffer
        let eflags_col_width = (ui.available_width() - gap - 20.0) / 2.0;

        let eflags = emu.reg_read(9).unwrap_or(0);
        let flags = [
            ("CF (Carry)", 0),
            ("PF (Parity)", 2),
            ("AF (Aux Carry)", 4),
            ("ZF (Zero)", 6),
            ("SF (Sign)", 7),
            ("OF (Overflow)", 11),
        ];

        egui::Grid::new("x86_eflags_grid")
            .num_columns(2)
            .striped(true)
            .min_col_width(eflags_col_width)
            .show(ui, |ui| {
                for (name, bit) in flags {
                    let val = (eflags >> bit) & 1;
                    let color = if val == 1 {
                        egui::Color32::RED
                    } else {
                        egui::Color32::DARK_GRAY
                    };
                    ui.label(name);
                    ui.colored_label(color, val.to_string());
                    ui.end_row();
                }
            });
    }

    fn is_instruction(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        match lower.as_str() {
            "add" | "adc" | "sub" | "sbb" | "cmp" | "test" | "and" | "or" | "xor" | "mul"
            | "div" | "inc" | "dec" | "mov" | "lea" | "push" | "pop" | "jmp" | "call" | "ret"
            | "hlt" | "nop" | "int" | "jo" | "jno" | "jb" | "jc" | "jnae" | "jae" | "jnb"
            | "jnc" | "je" | "jz" | "jne" | "jnz" | "jbe" | "jna" | "ja" | "jnbe" | "js"
            | "jns" | "jp" | "jpe" | "jnp" | "jpo" | "jl" | "jnge" | "jge" | "jnl" | "jle"
            | "jng" | "jg" | "jnle" | "cmovo" | "cmovno" | "cmovb" | "cmovc" | "cmovnae"
            | "cmovae" | "cmovnb" | "cmovnc" | "cmove" | "cmovz" | "cmovne" | "cmovnz"
            | "cmovbe" | "cmovna" | "cmova" | "cmovnbe" | "cmovs" | "cmovns" | "cmovp"
            | "cmovpe" | "cmovnp" | "cmovpo" | "cmovl" | "cmovnge" | "cmovge" | "cmovnl"
            | "cmovle" | "cmovng" | "cmovg" | "cmovnle" | "seto" | "setno" | "setb" | "setc"
            | "setnae" | "setae" | "setnb" | "setnc" | "sete" | "setz" | "setne" | "setnz"
            | "setbe" | "setna" | "seta" | "setnbe" | "sets" | "setns" | "setp" | "setpe"
            | "setnp" | "setpo" | "setl" | "setnge" | "setge" | "setnl" | "setle" | "setng"
            | "setg" | "setnle" => true,
            _ => false,
        }
    }

    fn is_register(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        match lower.as_str() {
            "rax" | "rcx" | "rdx" | "rbx" | "rsp" | "rbp" | "rsi" | "rdi" | "r8" | "r9" | "r10"
            | "r11" | "r12" | "r13" | "r14" | "r15" | "eax" | "ecx" | "edx" | "ebx" | "esp"
            | "ebp" | "esi" | "edi" | "r8d" | "r9d" | "r10d" | "r11d" | "r12d" | "r13d"
            | "r14d" | "r15d" | "ax" | "cx" | "dx" | "bx" | "sp" | "bp" | "si" | "di" | "al"
            | "cl" | "dl" | "bl" | "ah" | "ch" | "dh" | "bh" | "eip" | "rip" | "eflags"
            | "rflags" => true,
            _ => false,
        }
    }
}

pub mod device {
    use hyperemu::{EmuError, device::Device};

    pub struct X86Gpio {
        pub leds: u32,    // Offset 0x00 (Output)
        pub buttons: u32, // Offset 0x04 (Input)
    }

    impl X86Gpio {
        pub const fn new() -> Self {
            Self {
                leds: 0,
                buttons: 0,
            }
        }

        pub const fn set_button(&mut self, pin: u8, pressed: bool) {
            if pressed {
                self.buttons |= 1 << pin;
            } else {
                self.buttons &= !(1 << pin);
            }
        }

        pub const fn is_led_on(&self, pin: u8) -> bool {
            (self.leds & (1 << pin)) != 0
        }
    }

    impl Device for X86Gpio {
        #[inline(always)]
        fn read_32(&mut self, offset: u64) -> Result<u32, EmuError> {
            match offset {
                0x00 => Ok(self.leds),
                0x04 => Ok(self.buttons),
                _ => Ok(0),
            }
        }

        #[inline(always)]
        fn write_32(&mut self, offset: u64, val: u32) -> Result<(), EmuError> {
            match offset {
                0x00 => self.leds = val,
                // Offset 0x04 is Input-Only, so we ignore CPU writes to it!
                _ => {}
            }
            Ok(())
        }

        // Fallbacks to handle partial 16-bit and 8-bit reads/writes from the CPU
        #[inline(always)]
        fn read_16(&mut self, offset: u64) -> Result<u16, EmuError> {
            let val = self.read_32(offset & !3)?;
            Ok((val >> ((offset % 4) * 8)) as u16)
        }

        #[inline(always)]
        fn write_16(&mut self, offset: u64, val: u16) -> Result<(), EmuError> {
            let base = offset & !3;
            let shift = (offset % 4) * 8;
            let mask = !(0xFFFFu32 << shift);
            let current = self.read_32(base)?;
            self.write_32(base, (current & mask) | ((val as u32) << shift))
        }

        #[inline(always)]
        fn read_8(&mut self, offset: u64) -> Result<u8, EmuError> {
            let val = self.read_32(offset & !3)?;
            Ok((val >> ((offset % 4) * 8)) as u8)
        }

        #[inline(always)]
        fn write_8(&mut self, offset: u64, val: u8) -> Result<(), EmuError> {
            let base = offset & !3;
            let shift = (offset % 4) * 8;
            let mask = !(0xFFu32 << shift);
            let current = self.read_32(base)?;
            self.write_32(base, (current & mask) | ((val as u32) << shift))
        }
    }
}

pub struct X86GpioGui {
    pub name: String,
    pub device: Arc<Mutex<device::X86Gpio>>,
}

impl GuiPeripheral for X86GpioGui {
    fn name(&self) -> &str {
        &self.name
    }

    fn category(&self) -> crate::ui::peripherals::PeripheralCategory {
        crate::ui::peripherals::PeripheralCategory::Hardware
    }

    fn render(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(&self.name).strong());

                let mut dev = self.device.lock().unwrap();

                ui.horizontal(|ui| {
                    // Checkbox interacting with Offset 0x04
                    let mut is_pressed = (dev.buttons & 1) != 0;
                    if ui
                        .checkbox(&mut is_pressed, "Toggle Pin 0 (Input)")
                        .changed()
                    {
                        dev.set_button(0, is_pressed);
                    }

                    ui.add_space(20.0);

                    // LED interacting with Offset 0x00
                    let is_on = dev.is_led_on(0);
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
                        egui::RichText::new(format!("LEDS (0x00):    0x{:08X}", dev.leds))
                            .monospace()
                            .small(),
                    );
                    ui.label(
                        egui::RichText::new(format!("BUTTONS (0x04): 0x{:08X}", dev.buttons))
                            .monospace()
                            .small(),
                    );
                });
            });
        });
    }
}
