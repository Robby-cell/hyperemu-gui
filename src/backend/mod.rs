pub mod armv7;
pub mod x86;

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
    pub byte_size: usize,
}

impl DisassemblyInfo {
    pub const fn new(
        hex_bytes: String,
        disassembly: String,
        internal_enum: String,
        byte_size: usize,
    ) -> Self {
        Self {
            hex_bytes,
            disassembly,
            internal_enum,
            byte_size,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AssembleResult {
    pub bytes: Vec<u8>,
    pub entry_point: u64,
    /// Maps a label/function name to its exact physical byte address (IP)
    pub labels: HashMap<String, u64>,
    /// Total number of physical instructions and data directives emitted
    pub instruction_count: usize,
    /// Maps exact UI text line numbers to Physical Memory PCs
    pub line_to_pc: HashMap<usize, u64>,
    /// Maps Physical Memory PCs back to UI text lines for breakpoints
    pub pc_to_line: HashMap<u64, usize>,
}

pub trait ArchBackend {
    fn arch(&self) -> Arch;

    /// A stable, lowercase string ID used for serialization (e.g. "armv7", "x86")
    fn id(&self) -> &'static str;

    fn name(&self) -> &'static str;

    fn default_code(&self) -> &'static str;

    fn default_mode(&self) -> CpuMode;

    fn pc_reg(&self) -> usize;

    fn sp_reg(&self) -> usize;

    /// Total number of CPU registers available to be snapshotted
    fn num_registers(&self) -> usize;

    /// The architecture's standard machine word size in bytes (e.g., 4 for 32-bit, 8 for 64-bit)
    fn word_size(&self) -> usize;

    fn setup_startup_state(&self, emu: &mut HyperEmu);

    fn assemble(&self, code: &str) -> Result<AssembleResult, String>;

    /// Disassembles the instruction at `addr`.
    /// Returns (Hex Bytes String, Disassembly String, Internal Enum String, Instruction Byte Size)
    fn disassemble(&self, addr: u64, emu: &mut HyperEmu) -> DisassemblyInfo;

    fn render_registers(
        &self,
        ui: &mut egui::Ui,
        emu: &HyperEmu,
        prev_regs: &HashMap<usize, u64>,
        labels: &HashMap<u64, String>,
    );

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
