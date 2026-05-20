use super::{ArchBackend, AssembleResult, DisassemblyInfo};
use crate::backend::armv7::Stm32GpioGui;
use crate::ui::peripherals::GuiPeripheral;
use eframe::egui;
use hyperemu::{
    Arch, CpuMode, HyperEmu,
    arch::rv32i::decode::decode_riscv,
    device::{self, stm32_gpio::Stm32Gpio},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub struct Rv32iBackend;

const STARTER_CODE: &str = r#"; RV32I Demo Code
; Note: Real RISC-V Assembler not yet integrated into GUI.
; This backend is fully operational via binary blobs and the API.
"#;

impl ArchBackend for Rv32iBackend {
    fn arch(&self) -> Arch {
        Arch::Rv32i
    }

    fn id(&self) -> &'static str {
        "rv32i"
    }

    fn name(&self) -> &'static str {
        "RISC-V (RV32I)"
    }

    fn default_code(&self) -> &'static str {
        STARTER_CODE
    }

    fn default_mode(&self) -> CpuMode {
        CpuMode::MODE_32
    }

    fn pc_reg(&self) -> usize {
        32
    }

    fn sp_reg(&self) -> usize {
        2
    }

    fn num_registers(&self) -> usize {
        33
    }

    fn word_size(&self) -> usize {
        4
    }

    fn setup_startup_state(&self, emu: &mut HyperEmu) {
        emu.reg_write(self.sp_reg(), 0xB000).unwrap();
        emu.reg_write(self.pc_reg(), 0).unwrap();
    }

    fn assemble(&self, _code: &str) -> Result<AssembleResult, String> {
        // Mock assemble for the UI placeholder
        Ok(AssembleResult {
            bytes: vec![0x13, 0x00, 0x00, 0x00], // NOP (ADDI x0, x0, 0)
            entry_point: 0,
            labels: HashMap::new(),
            instruction_count: 1,
            line_to_pc: HashMap::new(),
            pc_to_line: HashMap::new(),
        })
    }

    fn disassemble(&self, addr: u64, emu: &mut HyperEmu) -> DisassemblyInfo {
        let mut bytes = [0u8; 4];
        if emu.bus.read_bytes(addr, &mut bytes).is_ok() {
            let raw = u32::from_le_bytes(bytes);
            let instr = decode_riscv(raw);
            let bytes_str = format!(
                "{:02X} {:02X} {:02X} {:02X}",
                bytes[0], bytes[1], bytes[2], bytes[3]
            );
            let internal = format!("{:?}", instr);
            let dis_str = internal
                .split('{')
                .next()
                .unwrap_or(&internal)
                .to_string()
                .to_uppercase();
            DisassemblyInfo::new(bytes_str, dis_str, internal, 4)
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
        let reg_names = [
            "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3",
            "a4", "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11",
            "t3", "t4", "t5", "t6", "pc",
        ];

        egui::Grid::new("riscv_reg_grid")
            .num_columns(4)
            .striped(true)
            .show(ui, |ui| {
                for (i, name) in reg_names.iter().enumerate() {
                    let val = emu.reg_read(i).unwrap_or(0);
                    let prev = prev_regs.get(&i).copied().unwrap_or(val);
                    let color = if val != prev {
                        egui::Color32::YELLOW
                    } else {
                        ui.visuals().text_color()
                    };

                    ui.label(format!("x{} ({})", i, name));
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

                    if i % 2 != 0 {
                        ui.end_row();
                    }
                }
            });
    }

    fn is_instruction(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        match lower.as_str() {
            "add" | "addi" | "sub" | "lui" | "auipc" | "jal" | "jalr" | "beq" | "bne" | "blt"
            | "bge" | "lw" | "sw" => true,
            _ => false,
        }
    }

    fn is_register(&self, word: &str) -> bool {
        let lower = word.to_lowercase();
        lower.starts_with('x') && lower[1..].parse::<u8>().is_ok()
    }
}
