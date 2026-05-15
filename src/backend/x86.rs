use super::{ArchBackend, AssembleResult, DisassemblyInfo};
use crate::ui::peripherals::GuiPeripheral;
use eframe::egui;
use hyperemu::{
    Arch, CpuMode, HyperEmu,
    arch::x86::decode::X86Decoder,
    device::{self},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use x86_translator::{assembler::Assembler, disassembler::Disassembler};

pub struct X86Backend;

const STARTER_CODE: &str = r#"; x86 32-bit GPIO Demo
.global _start
_start:
    ; Configure GPIO Base Address (0x40000000)
    MOV EAX, 0x40000000

    ; Set PA5 as output (MODER bit 10 = 1) -> 0x400
    MOV EBX, 0x400
    MOV [EAX], EBX

loop:
    ; 1. Read the IDR (Input Data Register) at offset 0x10
    MOV ECX, [EAX+0x10]
    
    ; 2. Mask out everything except bit 0
    AND ECX, 1
    
    ; 3. Is the button pressed?
    CMP ECX, 1
    JE turn_on

turn_off:
    MOV EDX, 0
    MOV [EAX+0x14], EDX
    JMP loop

turn_on:
    ; Turn LED ON (ODR bit 5 = 1 -> 0x20)
    MOV EDX, 0x20
    MOV [EAX+0x14], EDX
    JMP loop
"#;

impl ArchBackend for X86Backend {
    fn arch(&self) -> Arch {
        Arch::X86
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
        let res = Assembler::new()
            .bitness(32)
            .assemble(code)
            .map_err(|e| format!("{:?}", e))?;

        // 2. We need to map UI source-code lines to physical Program Counter addresses.
        // Because x86 is variable length, we use the emulator's internal decoder to measure the
        // byte lengths of the generated instructions.
        let mut pc_to_line = HashMap::new();
        let mut line_to_pc = HashMap::new();
        let mut current_pc = res.entry_point;

        let mut decoder = X86Decoder::new(&res.bytes);
        let mut inst_lengths = Vec::new();

        while decoder.consumed() < res.bytes.len() {
            let start = decoder.consumed();
            let _ = decoder.decode_instr(); // Parse just to advance the cursor
            let len = decoder.consumed() - start;
            if len == 0 {
                break;
            } // Failsafe
            inst_lengths.push(len);
        }

        // 3. Correlate lengths back to the non-empty lines in the editor
        let mut inst_idx = 0;
        for (i, line) in code.split('\n').enumerate() {
            let trimmed = line.trim();
            // Skip comments, empty lines, and labels/directives
            if trimmed.is_empty()
                || trimmed.starts_with(';')
                || trimmed.ends_with(':')
                || trimmed.starts_with('.')
            {
                continue;
            }

            if inst_idx < inst_lengths.len() {
                pc_to_line.insert(current_pc, i);
                line_to_pc.insert(i, current_pc);
                current_pc += inst_lengths[inst_idx] as u64;
                inst_idx += 1;
            }
        }

        Ok(AssembleResult {
            bytes: res.bytes,
            entry_point: res.entry_point,
            labels: res.labels,
            instruction_count: res.instruction_count,
            line_to_pc,
            pc_to_line,
        })
    }

    fn disassemble(&self, addr: u64, emu: &mut HyperEmu) -> DisassemblyInfo {
        let mut bytes = [0u8; 15]; // 15 bytes is the max instruction length for x86

        if emu.bus.read_bytes(addr, &mut bytes).is_ok() {
            // 1. Use the Emulator's internal AST to determine exact byte size
            let mut emu_decoder = X86Decoder::new(&bytes);
            let internal_ast = emu_decoder.decode_instr();

            let mut size = emu_decoder.consumed() as u64;
            if size == 0 {
                size = 1;
            } // Fallback for total garbage

            // Slice only the exact bytes that belong to THIS instruction
            let instr_bytes = &bytes[0..size as usize];

            let bytes_str = instr_bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");

            let internal_enum = format!("{:#?}", internal_ast);

            // 2. Use the x86_translator (iced_x86) to generate a beautiful Intel-syntax string
            let dis_str = match Disassembler::new()
                .bitness(32)
                .start_address(addr)
                .disassemble(instr_bytes)
            {
                Ok(results) if !results.is_empty() => results[0].clone(),
                _ => "???".to_string(), // Fallback if iced_x86 fails
            };

            DisassemblyInfo::new(bytes_str, dis_str, internal_enum, size)
        } else {
            DisassemblyInfo::new("??".into(), "Invalid Memory".into(), "".into(), 1)
        }
    }

    fn create_gpio(
        &self,
        name: String,
    ) -> (
        Arc<Mutex<dyn device::Device + Send>>,
        Arc<Mutex<dyn GuiPeripheral>>,
    ) {
        let _ = name;
        // let dev = Arc::new(Mutex::new(Stm32Gpio::new()));
        // let gui = Arc::new(Mutex::new(Stm32GpioGui {
        //     name,
        //     device: Arc::clone(&dev),
        // }));
        // (dev, gui)
        todo!("Not implemented")
    }

    fn render_registers(&self, ui: &mut egui::Ui, emu: &HyperEmu, prev_regs: &HashMap<usize, u64>) {
        egui::Grid::new("x86_reg_grid")
            .num_columns(2)
            .striped(true)
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

                    ui.label(*name);
                    ui.colored_label(
                        color,
                        egui::RichText::new(format!("0x{:08X}", val)).monospace(),
                    );
                    ui.end_row();
                }
            });

        ui.separator();
        ui.heading("EFLAGS");

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
            // Base math / logic
            "add" | "adc" | "sub" | "sbb" | "cmp" | "test" | 
            "and" | "or" | "xor" | "mul" | "div" | "inc" | "dec" |
            // Movement / Addresses
            "mov" | "lea" | "push" | "pop" |
            // Control Flow & System
            "jmp" | "call" | "ret" | "hlt" | "nop" | "int" |
            // Conditional Jumps
            "jo" | "jno" | "jb" | "jc" | "jnae" | "jae" | "jnb" | "jnc" | 
            "je" | "jz" | "jne" | "jnz" | "jbe" | "jna" | "ja" | "jnbe" | 
            "js" | "jns" | "jp" | "jpe" | "jnp" | "jpo" | "jl" | "jnge" | 
            "jge" | "jnl" | "jle" | "jng" | "jg" | "jnle" |
            // Conditional Moves (CMOV)
            "cmovo" | "cmovno" | "cmovb" | "cmovc" | "cmovnae" | "cmovae" | 
            "cmovnb" | "cmovnc" | "cmove" | "cmovz" | "cmovne" | "cmovnz" | 
            "cmovbe" | "cmovna" | "cmova" | "cmovnbe" | "cmovs" | "cmovns" | 
            "cmovp" | "cmovpe" | "cmovnp" | "cmovpo" | "cmovl" | "cmovnge" | 
            "cmovge" | "cmovnl" | "cmovle" | "cmovng" | "cmovg" | "cmovnle" |
            // Conditional Sets (SET)
            "seto" | "setno" | "setb" | "setc" | "setnae" | "setae" | 
            "setnb" | "setnc" | "sete" | "setz" | "setne" | "setnz" | 
            "setbe" | "setna" | "seta" | "setnbe" | "sets" | "setns" | 
            "setp" | "setpe" | "setnp" | "setpo" | "setl" | "setnge" | 
            "setge" | "setnl" | "setle" | "setng" | "setg" | "setnle" => true,
            _ => false,
        }
    }

    fn is_register(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        match lower.as_str() {
            // 64-bit
            "rax" | "rcx" | "rdx" | "rbx" | "rsp" | "rbp" | "rsi" | "rdi" |
            "r8"  | "r9"  | "r10" | "r11" | "r12" | "r13" | "r14" | "r15" |
            // 32-bit
            "eax" | "ecx" | "edx" | "ebx" | "esp" | "ebp" | "esi" | "edi" |
            "r8d" | "r9d" | "r10d"| "r11d"| "r12d"| "r13d"| "r14d"| "r15d"|
            // 16-bit
            "ax" | "cx" | "dx" | "bx" | "sp" | "bp" | "si" | "di" |
            // 8-bit
            "al" | "cl" | "dl" | "bl" | "ah" | "ch" | "dh" | "bh" |
            // System / Special Status Registers
            "eip" | "rip" | "eflags" | "rflags" => true,
            _ => false,
        }
    }
}
