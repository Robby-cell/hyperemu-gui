use crate::backend::x86::X86Backend;
use crate::backend::{ArchBackend, armv7::Armv7Backend};
use crate::ui::peripherals::GuiPeripheral;
use eframe::egui;
use hyperemu::HyperEmu;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
#[repr(transparent)]
pub struct ClockSpeed(pub u64);

impl ClockSpeed {
    pub const fn new_hz(hz: u64) -> Self {
        Self(hz)
    }

    pub const fn new_khz(khz: u64) -> Self {
        Self::new_hz(khz * 1_000)
    }

    pub const fn new_mhz(mhz: u64) -> Self {
        Self::new_hz(mhz * 1_000_000)
    }

    pub const fn hz(&self) -> u64 {
        self.0
    }

    /// Calculates how many CPU cycles should execute within a given physical time duration
    pub fn cycles_in_duration(&self, duration: Duration) -> u64 {
        let nanos = duration.as_nanos();
        // (nanos * Hz) / 1,000,000,000 ns
        ((nanos * self.0 as u128) / 1_000_000_000) as u64
    }

    /// Calculates how much physical time it takes to execute a specific number of cycles
    pub fn duration_for_cycles(&self, cycles: u64) -> Duration {
        if self.0 == 0 {
            return Duration::ZERO;
        }
        let nanos = (cycles as u128 * 1_000_000_000) / (self.0 as u128);
        Duration::from_nanos(nanos as u64)
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct BackendWorkspace {
    pub code: String,
    pub active_maps: Vec<MemMapRecord>,
    pub clock_speed: ClockSpeed,
}

impl Default for BackendWorkspace {
    fn default() -> Self {
        Self {
            code: String::new(),
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
                    name: "Main Terminal".into(),
                    start: 0x10000000,
                    size: 0x1000,
                    dev_type: DeviceType::Uart,
                },
                MemMapRecord {
                    name: "User LED".into(),
                    start: 0x40000000,
                    size: 0x1000,
                    dev_type: DeviceType::Gpio,
                },
            ],
            clock_speed: ClockSpeed::new_khz(10),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct WorkspaceState {
    pub active_backend_id: String,
    pub workspaces: HashMap<String, BackendWorkspace>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            active_backend_id: "armv7".into(),
            workspaces: HashMap::new(),
        }
    }
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

#[derive(PartialEq)]
pub enum MobileTab {
    Editor,
    Cpu,
    Hardware,
    Consoles,
    Memory,
}

pub struct EmuApp {
    pub backends: Vec<Arc<dyn ArchBackend>>,
    pub active_backend: usize,

    pub workspaces: HashMap<String, BackendWorkspace>,

    pub code: String,
    pub emu: Option<HyperEmu>,
    pub is_running: bool,
    pub error_msg: Option<String>,

    pub clock_speed: ClockSpeed,
    pub unconsumed_time: Duration,

    pub left_tab: LeftTab,
    pub central_tab: CentralTab,
    pub mobile_tab: MobileTab,

    // Debugging & Highlighting
    pub prev_regs: HashMap<usize, u64>,
    pub prev_stack: HashMap<u64, u32>,
    pub pc_to_line: HashMap<u64, usize>,
    pub line_to_pc: HashMap<usize, u64>,
    pub labels: HashMap<u64, String>,

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
        let backends: Vec<Arc<dyn ArchBackend>> =
            vec![Arc::new(Armv7Backend), Arc::new(X86Backend)];
        let code = backends[0].default_code().to_string();

        Self {
            backends,
            active_backend: 0,
            workspaces: HashMap::new(),
            code,
            emu: None,
            is_running: false,
            error_msg: None,
            clock_speed: ClockSpeed::new_khz(10),
            unconsumed_time: Duration::ZERO,
            left_tab: LeftTab::Hardware,
            central_tab: CentralTab::Editor,
            mobile_tab: MobileTab::Editor,

            prev_regs: HashMap::new(),
            prev_stack: HashMap::new(),
            pc_to_line: HashMap::new(),
            line_to_pc: HashMap::new(),
            labels: HashMap::new(),

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

impl EmuApp {
    /// Syncs the active UI values back into the `workspaces` map using the stable ID
    pub fn sync_workspace(&mut self) {
        let backend_id = self.current_backend().id().to_string();
        let ws = BackendWorkspace {
            code: self.code.clone(),
            active_maps: self.active_maps.clone(),
            clock_speed: self.clock_speed,
        };
        self.workspaces.insert(backend_id, ws);
    }

    /// Handles switching to a new architecture cleanly
    pub fn switch_backend(&mut self, new_idx: usize) {
        self.sync_workspace();

        self.active_backend = new_idx;
        let backend_id = self.backends[new_idx].id().to_string(); // Use ID instead of name

        if let Some(ws) = self.workspaces.get(&backend_id) {
            self.code = ws.code.clone();
            self.active_maps = ws.active_maps.clone();
            self.clock_speed = ws.clock_speed;
        } else {
            self.code = self.backends[new_idx].default_code().to_string();
            let def_ws = BackendWorkspace::default();
            self.active_maps = def_ws.active_maps;
            self.clock_speed = def_ws.clock_speed;
        }

        self.emu = None;
        self.gui_peripherals.clear();
        self.error_msg = None;
        self.is_running = false;
        self.prev_regs.clear();
        self.prev_stack.clear();
        self.pc_to_line.clear();
        self.line_to_pc.clear();
        self.labels.clear();
        self.breakpoints.lock().unwrap().clear();
    }

    /// Applies a completely loaded WorkspaceState
    pub fn apply_workspace_state(&mut self, state: WorkspaceState) {
        self.workspaces = state.workspaces;

        // Safely resolve the string ID back to an array index
        let mut target_idx = 0; // Default fallback
        for (i, backend) in self.backends.iter().enumerate() {
            if backend.id() == state.active_backend_id {
                target_idx = i;
                break;
            }
        }

        self.active_backend = target_idx;
        let backend_id = self.backends[target_idx].id().to_string();

        if let Some(ws) = self.workspaces.get(&backend_id) {
            self.code = ws.code.clone();
            self.active_maps = ws.active_maps.clone();
            self.clock_speed = ws.clock_speed;
        } else {
            self.code = self.backends[target_idx].default_code().to_string();
            let def_ws = BackendWorkspace::default();
            self.active_maps = def_ws.active_maps;
            self.clock_speed = def_ws.clock_speed;
        }

        self.emu = None;
        self.gui_peripherals.clear();
        self.error_msg = None;
        self.is_running = false;
        self.labels.clear();
    }
}

impl eframe::App for EmuApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.sync_workspace();
        let state = WorkspaceState {
            active_backend_id: self.current_backend().id().to_string(),
            workspaces: self.workspaces.clone(),
        };
        if let Ok(json) = serde_json::to_string(&state) {
            storage.set_string(eframe::APP_KEY, json);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let state = if let Ok(mut pending) = self.pending_load.lock() {
            pending.take()
        } else {
            None
        };
        if let Some(state) = state {
            self.apply_workspace_state(state);
        }

        crate::ui::render_layout(self, ui);

        if self.is_running {
            // 1. Get raw UI delta time and convert cleanly into a Rust Duration
            let dt_secs = ui.input(|i| i.unstable_dt).max(0.001);
            let dt = Duration::from_secs_f32(dt_secs);

            // 2. Accumulate real physical time
            self.unconsumed_time += dt;

            // 3. Ask our domain object how many cycles we should execute in this time frame
            let mut batch = self.clock_speed.cycles_in_duration(self.unconsumed_time) as usize;

            // Cap at 500k so we don't freeze the UI on max speed
            if batch > 500_000 {
                batch = 500_000;
                // Cap the unconsumed time to match, so it doesn't wind up infinitely in the background
                self.unconsumed_time = self.clock_speed.duration_for_cycles(500_000);
            }

            if batch > 0 {
                // Remove ONLY the exact time it took to run this batch
                self.unconsumed_time -= self.clock_speed.duration_for_cycles(batch as u64);
                self.snapshot_registers();

                if let Some(emu) = &mut self.emu {
                    match emu.step_batch(batch as _) {
                        Ok(_) => ui.ctx().request_repaint(),
                        Err(hyperemu::EmuError::Breakpoint(_)) => {
                            self.is_running = false;
                        }
                        Err(e) => {
                            self.error_msg = Some(format!("Runtime Error: {:?}", e));
                            self.is_running = false;
                        }
                    }
                }
            } else {
                // Need to request repaint if the clock speed is extremely slow (e.g. 1 Hz)
                ui.ctx().request_repaint();
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
                // We use our new helper method to safely unpack the HashMaps and String IDs!
                app.apply_workspace_state(state);
            }
        }

        app
    }

    pub fn trigger_save(&mut self) {
        self.sync_workspace();
        let state = WorkspaceState {
            active_backend_id: self.current_backend().id().to_string(),
            workspaces: self.workspaces.clone(),
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
        let backend = self.current_backend();
        let sp_reg = backend.sp_reg();
        let num_regs = backend.num_registers();
        let word_size = backend.word_size() as u64;

        if let Some(emu) = &mut self.emu {
            // 1. Snapshot exact CPU Registers dynamically
            for i in 0..num_regs {
                if let Ok(val) = emu.reg_read(i) {
                    self.prev_regs.insert(i, val);
                }
            }

            // 2. Snapshot the Stack (Top 16 Words) dynamically based on Word Size
            let sp = emu.reg_read(sp_reg).unwrap_or(0);
            self.prev_stack.clear();

            for i in 0..16 {
                let addr = sp.wrapping_add(i * word_size);

                // Read 32-bit data (If we add a 64-bit Arch later, we can branch to bus.read_64 here)
                if let Ok(val) = emu.bus.read_32(addr) {
                    self.prev_stack.insert(addr, val);
                }
            }
        }
    }
}
