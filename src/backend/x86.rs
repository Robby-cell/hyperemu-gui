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

const STARTER_CODE: &str = r#".text
.global _start
_start:
    ; UART base address
    mov ebx, 0x10000000

    ; Pointer to string
    mov ebp, message
print_loop:
    mov al, byte ptr [ebp]
    ; null terminator?
    test al, al
    jz finished_printing

    ; write byte to UART DATA register
    mov byte ptr [ebx], al
    add ebp, 1
    jmp print_loop

finished_printing:
    nop
done:
    jmp done

message:
	.ascii "Hello, World!\n"
_message_null: .byte 0
    .align 4
message_len: .dword _message_null - message
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
        // Removed: The PC mapping feature that tried to loop line-by-line and parse lengths.
        let res = Assembler::new()
            .bitness(32)
            .assemble(code)
            .map_err(|e| format!("{}", e))?; // Changed from {:?} to {} to render the Display message cleanly

        println!("{:?} | {:?}", res.line_to_ip, res.ip_to_line);

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

            let mut size = emu_decoder.consumed() as u64;
            if size == 0 {
                size = 1;
            }

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
    ) -> (
        Arc<Mutex<dyn device::Device + Send>>,
        Arc<Mutex<dyn GuiPeripheral>>,
    ) {
        let _ = name;
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
