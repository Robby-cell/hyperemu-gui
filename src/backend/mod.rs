pub mod armv7;

use crate::ui::peripherals::GuiPeripheral;
use eframe::egui;
use hyperemu::{Arch, CpuMode, HyperEmu, device};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub struct DisassemblyInfo {
    pub hex_bytes: String,
    pub disassembly: String,
    pub internal_enum: String,
    pub byte_size: u64,
}

impl DisassemblyInfo {
    pub const fn new(
        hex_bytes: String,
        disassembly: String,
        internal_enum: String,
        byte_size: u64,
    ) -> Self {
        Self {
            hex_bytes,
            disassembly,
            internal_enum,
            byte_size,
        }
    }
}

pub trait ArchBackend {
    fn arch(&self) -> Arch;

    fn name(&self) -> &'static str;

    fn default_code(&self) -> &'static str;

    fn default_mode(&self) -> CpuMode;

    fn pc_reg(&self) -> usize;

    fn sp_reg(&self) -> usize;

    fn setup_startup_state(&self, emu: &mut HyperEmu);

    fn assemble(&self, code: &str) -> Result<Vec<u8>, String>;

    fn build_pc_map(&self, code: &str) -> (HashMap<u64, usize>, HashMap<usize, u64>);

    /// Disassembles the instruction at `addr`.
    /// Returns (Hex Bytes String, Disassembly String, Internal Enum String, Instruction Byte Size)
    fn disassemble(&self, addr: u64, emu: &mut HyperEmu) -> DisassemblyInfo;

    fn render_registers(&self, ui: &mut egui::Ui, emu: &HyperEmu, prev_regs: &HashMap<usize, u64>);

    /// Returns the CPU's memory bus peripheral mapped simultaneously as an emulator device and a renderable GUI component
    fn create_gpio(
        &self,
        name: String,
    ) -> (
        Arc<Mutex<dyn device::Device + Send>>,
        Arc<Mutex<dyn GuiPeripheral>>,
    );

    fn is_instruction(&self, word: &str) -> bool;

    fn is_register(&self, word: &str) -> bool;
}
