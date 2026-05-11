use crate::backend::{ArchBackend, armv7::Armv7Backend};
use crate::ui::peripherals::GuiPeripheral;
use eframe::egui;
use hyperemu::HyperEmu;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DeviceType {
    Ram,
    Gpio,
    Uart,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MemMapRecord {
    pub name: String,
    pub start: u64,
    pub size: u64,
    pub dev_type: DeviceType,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)] // Allows adding new fields in the future without breaking old saves
pub struct WorkspaceState {
    pub code: String,
    pub active_backend: usize,
    pub active_maps: Vec<MemMapRecord>,
}

#[derive(PartialEq)]
pub enum LeftTab {
    Hardware,
    Consoles,
    MemoryMap,
}

#[derive(PartialEq)]
pub enum CentralTab {
    Editor,
    Disassembly,
    MemoryView,
}

pub struct EmuApp {
    pub backends: Vec<Arc<dyn ArchBackend>>,
    pub active_backend: usize,

    pub code: String,
    pub emu: Option<HyperEmu>,
    pub is_running: bool,
    pub error_msg: Option<String>,

    pub left_tab: LeftTab,
    pub central_tab: CentralTab,

    // Debugging & Highlighting
    pub prev_regs: HashMap<usize, u64>,
    pub prev_stack: HashMap<u64, u32>,
    pub pc_to_line: HashMap<u64, usize>,
    pub line_to_pc: HashMap<usize, u64>,
    pub breakpoints: Arc<Mutex<HashSet<u64>>>,
    pub breakpoint_input: String,
    pub ignore_next_bp: Arc<Mutex<Option<u64>>>,

    // Dynamic Peripherals
    pub active_maps: Vec<MemMapRecord>,
    pub gui_peripherals: Vec<Arc<Mutex<dyn GuiPeripheral>>>,

    pub map_input_name: String,
    pub map_input_addr: String,
    pub map_input_size: String,
    pub map_input_type: DeviceType,

    pub memory_base_input: String,
    pub memory_base_addr: u64,
    pub pending_load: Arc<Mutex<Option<WorkspaceState>>>,
}

impl Default for EmuApp {
    fn default() -> Self {
        let backends: Vec<Arc<dyn ArchBackend>> = vec![Arc::new(Armv7Backend)];
        let code = backends[0].default_code().to_string();

        Self {
            backends,
            active_backend: 0,
            code,
            emu: None,
            is_running: false,
            error_msg: None,
            left_tab: LeftTab::Hardware,
            central_tab: CentralTab::Editor,
            prev_regs: HashMap::new(),
            prev_stack: HashMap::new(),
            pc_to_line: HashMap::new(),
            line_to_pc: HashMap::new(),
            breakpoints: Arc::new(Mutex::new(HashSet::new())),
            breakpoint_input: String::new(),
            ignore_next_bp: Arc::new(Mutex::new(None)),
            active_maps: vec![
                MemMapRecord {
                    name: "Code".into(),
                    start: 0x0000,
                    size: 0x4000,
                    dev_type: DeviceType::Ram,
                },
                MemMapRecord {
                    name: "Stack".into(),
                    start: 0x8000,
                    size: 0x4000,
                    dev_type: DeviceType::Ram,
                },
                MemMapRecord {
                    name: "User LED".into(),
                    start: 0x40000000,
                    size: 0x1000,
                    dev_type: DeviceType::Gpio,
                },
                MemMapRecord {
                    name: "Main Terminal".into(),
                    start: 0x10000000,
                    size: 0x1000,
                    dev_type: DeviceType::Uart,
                },
            ],
            gui_peripherals: Vec::new(),
            map_input_name: "Peripheral".to_string(),
            map_input_addr: "0x20000000".to_string(),
            map_input_size: "0x1000".to_string(),
            map_input_type: DeviceType::Gpio,
            memory_base_input: "0x00000000".to_string(),
            memory_base_addr: 0,
            pending_load: Arc::new(Mutex::new(None)),
        }
    }
}

impl eframe::App for EmuApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let state = WorkspaceState {
            code: self.code.clone(),
            active_backend: self.active_backend,
            active_maps: self.active_maps.clone(),
        };
        eframe::set_value(storage, eframe::APP_KEY, &state);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Ok(mut pending) = self.pending_load.lock() {
            if let Some(state) = pending.take() {
                self.code = state.code;
                self.active_backend = state.active_backend;
                self.active_maps = state.active_maps;
                self.emu = None;
            }
        }

        crate::ui::render_layout(self, ui);

        if self.is_running {
            self.snapshot_registers();
            if let Some(emu) = &mut self.emu {
                match emu.step_batch(256) {
                    Ok(_) => ui.ctx().request_repaint(),
                    Err(hyperemu::EmuError::Breakpoint(_)) => self.is_running = false,
                    Err(e) => {
                        self.error_msg = Some(format!("Runtime Error: {:?}", e));
                        self.is_running = false;
                    }
                }
            }
        }
    }
}

impl EmuApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self::default(); // Start with defaults

        // Attempt to load from storage
        if let Some(storage) = cc.storage {
            if let Some(state) = eframe::get_value::<WorkspaceState>(storage, eframe::APP_KEY) {
                // If we found a saved state, overwrite the defaults
                app.code = state.code;
                app.active_backend = state.active_backend;

                // Only restore maps if the array isn't empty, otherwise keep defaults
                if !state.active_maps.is_empty() {
                    app.active_maps = state.active_maps;
                }
            }
        }

        app
    }

    pub fn trigger_save(&self) {
        let state = WorkspaceState {
            code: self.code.clone(),
            active_backend: self.active_backend,
            active_maps: self.active_maps.clone(),
        };

        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("JSON Workspace", &["json"])
                .set_file_name("workspace.json")
                .save_file()
            {
                if let Ok(json) = serde_json::to_string_pretty(&state) {
                    let _ = std::fs::write(path, json);
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(handle) = rfd::AsyncFileDialog::new()
                    .add_filter("JSON Workspace", &["json"])
                    .set_file_name("workspace.json")
                    .save_file()
                    .await
                {
                    if let Ok(json) = serde_json::to_string_pretty(&state) {
                        let _ = handle.write(json.as_bytes()).await;
                    }
                }
            });
        }
    }

    pub fn trigger_load(&self) {
        let pending = Arc::clone(&self.pending_load);

        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("JSON Workspace", &["json"])
                .pick_file()
            {
                if let Ok(data) = std::fs::read_to_string(path) {
                    if let Ok(state) = serde_json::from_str::<WorkspaceState>(&data) {
                        *pending.lock().unwrap() = Some(state);
                    }
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            wasm_bindgen_futures::spawn_local(async move {
                if let Some(file) = rfd::AsyncFileDialog::new()
                    .add_filter("JSON Workspace", &["json"])
                    .pick_file()
                    .await
                {
                    let data = file.read().await;
                    if let Ok(text) = String::from_utf8(data) {
                        if let Ok(state) = serde_json::from_str::<WorkspaceState>(&text) {
                            *pending.lock().unwrap() = Some(state);
                        }
                    }
                }
            });
        }
    }

    pub fn current_backend(&self) -> Arc<dyn ArchBackend> {
        self.backends[self.active_backend].clone()
    }

    pub fn snapshot_registers(&mut self) {
        let sp_reg = self.current_backend().sp_reg();

        if let Some(emu) = &mut self.emu {
            // 1. Snapshot CPU Registers
            for i in 0..32 {
                if let Ok(val) = emu.reg_read(i) {
                    self.prev_regs.insert(i, val);
                }
            }

            // 2. Snapshot the Stack (Top 16 Words)
            let sp = emu.reg_read(sp_reg).unwrap_or(0);
            self.prev_stack.clear();

            for i in 0..16 {
                let addr = sp + (i * 4) as u64; // ARM words are 4 bytes
                if let Ok(val) = emu.bus.read_32(addr) {
                    self.prev_stack.insert(addr, val);
                }
            }
        }
    }

    pub fn build_pc_map(&mut self) {
        let (pc_to_line, line_to_pc) = self.current_backend().build_pc_map(&self.code);
        self.pc_to_line = pc_to_line;
        self.line_to_pc = line_to_pc;
    }
}
