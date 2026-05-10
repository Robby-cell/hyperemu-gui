use eframe::egui;
use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum PeripheralCategory {
    Hardware,
    Console,
}

// Adding `Send` ensures that traits map easily across standard multithreading bounds in eframe
pub trait GuiPeripheral: Send {
    fn name(&self) -> &str;
    fn category(&self) -> PeripheralCategory;
    fn render(&mut self, ui: &mut egui::Ui);
}

pub struct UartGui {
    pub name: String,
    pub buffer: Arc<Mutex<String>>,
}

impl GuiPeripheral for UartGui {
    fn name(&self) -> &str {
        &self.name
    }

    fn category(&self) -> PeripheralCategory {
        PeripheralCategory::Console
    }

    fn render(&mut self, ui: &mut egui::Ui) {
        ui.heading(&self.name);
        ui.horizontal(|ui| {
            if ui.button(format!("Clear {}", self.name)).clicked() {
                self.buffer.lock().unwrap().clear();
            }
        });

        egui::ScrollArea::vertical()
            .id_salt(format!("uart_scroll_{}", self.name))
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let mut text = self.buffer.lock().unwrap().clone();
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .interactive(false),
                );
            });
        ui.separator();
    }
}

#[derive(Clone)]
pub struct GuiUartWriter {
    pub buffer: Arc<Mutex<String>>,
}

impl Write for GuiUartWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer
            .lock()
            .unwrap()
            .push_str(&String::from_utf8_lossy(buf));
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
