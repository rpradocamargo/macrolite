#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use enigo::{Button as EButton, Coordinate, Direction, Enigo, Key as EKey, Keyboard, Mouse, Settings};
use rdev::{listen, Button, Event, EventType, Key};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, Sender}}, thread, time::{Duration, Instant}};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Action {
    MouseMove { x: f64, y: f64, delay_ms: u64 },
    MouseButton { button: String, down: bool, x: f64, y: f64, delay_ms: u64 },
    Key { key: String, down: bool, text: Option<String>, delay_ms: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hotkey { StartRecording, StopRecording, StartPlayback, StopPlayback }

#[derive(Debug, Clone)]
struct Shortcut {
    key: String,
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

impl Shortcut {
    fn new(key: &str, ctrl: bool, shift: bool, alt: bool, meta: bool) -> Self {
        Self { key: key.to_string(), ctrl, shift, alt, meta }
    }

    fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl { parts.push("Ctrl"); }
        if self.shift { parts.push("Shift"); }
        if self.alt { parts.push("Option"); }
        if self.meta { parts.push("Command"); }
        parts.push(match self.key.as_str() {
            "KeyR" => "R", "KeyS" => "S", "KeyP" => "P", "KeyX" => "X",
            other => other,
        });
        parts.join(" + ")
    }

    fn matches(&self, key: &Key, keys: &HashSet<Key>) -> bool {
        self.key == format!("{key:?}")
            && self.ctrl == (keys.contains(&Key::ControlLeft) || keys.contains(&Key::ControlRight))
            && self.shift == (keys.contains(&Key::ShiftLeft) || keys.contains(&Key::ShiftRight))
            && self.alt == (keys.contains(&Key::Alt) || keys.contains(&Key::AltGr))
            && self.meta == (keys.contains(&Key::MetaLeft) || keys.contains(&Key::MetaRight))
    }
}

#[derive(Debug, Clone)]
struct HotkeyConfig {
    start_recording: Shortcut,
    stop_recording: Shortcut,
    start_playback: Shortcut,
    stop_playback: Shortcut,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            start_recording: Shortcut::new("KeyR", true, true, false, false),
            stop_recording: Shortcut::new("KeyS", true, true, false, false),
            start_playback: Shortcut::new("KeyP", true, true, false, false),
            stop_playback: Shortcut::new("KeyX", true, true, false, false),
        }
    }
}

impl HotkeyConfig {
    fn get(&self, hotkey: Hotkey) -> &Shortcut {
        match hotkey {
            Hotkey::StartRecording => &self.start_recording,
            Hotkey::StopRecording => &self.stop_recording,
            Hotkey::StartPlayback => &self.start_playback,
            Hotkey::StopPlayback => &self.stop_playback,
        }
    }

    fn get_mut(&mut self, hotkey: Hotkey) -> &mut Shortcut {
        match hotkey {
            Hotkey::StartRecording => &mut self.start_recording,
            Hotkey::StopRecording => &mut self.stop_recording,
            Hotkey::StartPlayback => &mut self.start_playback,
            Hotkey::StopPlayback => &mut self.stop_playback,
        }
    }
}

#[derive(Debug)]
enum RecorderMessage { Event(Event), Hotkey(Hotkey), PlaybackFinished(String), Error(String) }

struct MacroLite {
    actions: Vec<Action>,
    recording: bool,
    playing: bool,
    repeat_count: usize,
    repeat_forever: bool,
    speed_index: usize,
    status: String,
    rx: Receiver<RecorderMessage>,
    tx: Sender<RecorderMessage>,
    recording_flag: Arc<AtomicBool>,
    last_event: Option<Instant>,
    last_mouse: Option<(f64, f64)>,
    last_recorded_mouse: Option<(f64, f64)>,
    last_move_recorded: Option<Instant>,
    playback_stop: Arc<AtomicBool>,
    macro_path: Option<std::path::PathBuf>,
    hotkeys: Arc<Mutex<HotkeyConfig>>,
    capture_hotkey: Arc<Mutex<Option<Hotkey>>>,
}

impl Default for MacroLite {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        let listener_tx = tx.clone();
        let hotkeys = Arc::new(Mutex::new(HotkeyConfig::default()));
        let capture_hotkey = Arc::new(Mutex::new(None));
        let listener_hotkeys = hotkeys.clone();
        let listener_capture = capture_hotkey.clone();
        thread::spawn(move || {
            let error_tx = listener_tx.clone();
            let pressed = Arc::new(Mutex::new(HashSet::<Key>::new()));
            let callback_pressed = pressed.clone();
            let callback_tx = listener_tx.clone();
            let callback = move |event: Event| {
                let mut hotkey_key = false;
                if let Ok(mut keys) = callback_pressed.lock() {
                    match &event.event_type {
                        EventType::KeyPress(key) => {
                            keys.insert(*key);
                            let is_modifier = matches!(key,
                                Key::ControlLeft | Key::ControlRight |
                                Key::ShiftLeft | Key::ShiftRight |
                                Key::Alt | Key::AltGr | Key::MetaLeft | Key::MetaRight
                            );

                            if !is_modifier {
                                let capture_target = listener_capture.lock().ok().and_then(|mut target| target.take());
                                if let Some(target) = capture_target {
                                    if let Ok(mut config) = listener_hotkeys.lock() {
                                        let binding = config.get_mut(target);
                                        binding.key = format!("{key:?}");
                                        binding.ctrl = keys.contains(&Key::ControlLeft) || keys.contains(&Key::ControlRight);
                                        binding.shift = keys.contains(&Key::ShiftLeft) || keys.contains(&Key::ShiftRight);
                                        binding.alt = keys.contains(&Key::Alt) || keys.contains(&Key::AltGr);
                                        binding.meta = keys.contains(&Key::MetaLeft) || keys.contains(&Key::MetaRight);
                                    }
                                    hotkey_key = true;
                                } else if let Ok(config) = listener_hotkeys.lock() {
                                    let matched = [
                                        (Hotkey::StartRecording, config.get(Hotkey::StartRecording)),
                                        (Hotkey::StopRecording, config.get(Hotkey::StopRecording)),
                                        (Hotkey::StartPlayback, config.get(Hotkey::StartPlayback)),
                                        (Hotkey::StopPlayback, config.get(Hotkey::StopPlayback)),
                                    ].iter().find(|(_, shortcut)| shortcut.matches(key, &keys)).map(|(hotkey, _)| *hotkey);
                                    if let Some(hotkey) = matched {
                                        let _ = callback_tx.send(RecorderMessage::Hotkey(hotkey));
                                        hotkey_key = true;
                                    }
                                }
                            }
                        }
                        EventType::KeyRelease(key) => { keys.remove(key); }
                        _ => {}
                    }
                }
                // Shortcut keys are omitted from the recorded macro.
                if !hotkey_key { let _ = callback_tx.send(RecorderMessage::Event(event)); }
            };
            if let Err(error) = listen(callback) {
                let _ = error_tx.send(RecorderMessage::Error(format!("Input listener error: {error:?}")));
            }
        });
        Self {
            actions: Vec::new(), recording: false, playing: false,
            repeat_count: 5, repeat_forever: false, speed_index: 0, status: "Ready".into(), rx, tx,
            recording_flag: Arc::new(AtomicBool::new(false)),
            last_event: None, last_mouse: None, last_recorded_mouse: None, last_move_recorded: None,
            playback_stop: Arc::new(AtomicBool::new(false)), macro_path: None,
            hotkeys, capture_hotkey,
        }
    }
}

impl MacroLite {
    fn start_recording(&mut self) {
        if self.playing { return; }
        self.actions.clear();
        self.last_event = None;
        self.last_mouse = None;
        self.last_recorded_mouse = None;
        self.last_move_recorded = None;
        self.recording = true;
        self.recording_flag.store(true, Ordering::SeqCst);
        self.status = "Recording input… click Stop when finished".into();
    }

    fn stop_recording(&mut self) {
        self.recording = false;
        self.recording_flag.store(false, Ordering::SeqCst);
        self.status = format!("Recorded {} actions", self.actions.len());
    }

    fn process_events(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            let event = match message {
                RecorderMessage::Event(event) => event,
                RecorderMessage::Hotkey(hotkey) => {
                    match hotkey {
                        Hotkey::StartRecording => self.start_recording(),
                        Hotkey::StopRecording => if self.recording { self.stop_recording(); },
                        Hotkey::StartPlayback => self.play(),
                        Hotkey::StopPlayback => if self.playing {
                            self.playback_stop.store(true, Ordering::SeqCst);
                            self.status = "Stopping playback…".into();
                        },
                    }
                    continue;
                }
                RecorderMessage::PlaybackFinished(message) => {
                    self.playing = false;
                    self.playback_stop.store(false, Ordering::SeqCst);
                    self.status = message;
                    continue;
                }
                RecorderMessage::Error(error) => { self.status = format!("Input listener error: {error}"); continue; }
            };
            if !self.recording { continue; }
            let now = Instant::now();
            match event.event_type {
                EventType::MouseMove { x, y } => {
                    // Capture fine-grained movement. The previous version discarded many
                    // points and also advanced the timing clock for discarded events,
                    // which made playback dramatically faster than the original actions.
                    let moved = self.last_recorded_mouse
                        .map(|(px, py)| (px - x).abs() + (py - y).abs() >= 1.0)
                        .unwrap_or(true);
                    let elapsed = self.last_move_recorded
                        .map(|t| now.duration_since(t) >= Duration::from_millis(8))
                        .unwrap_or(true);
                    self.last_mouse = Some((x, y));
                    if moved && elapsed {
                        let delay_ms = self.last_event
                            .map(|t| now.duration_since(t).as_millis().min(60_000) as u64)
                            .unwrap_or(0);
                        self.actions.push(Action::MouseMove { x, y, delay_ms });
                        self.last_event = Some(now);
                        self.last_recorded_mouse = Some((x, y));
                        self.last_move_recorded = Some(now);
                    }
                }
                EventType::ButtonPress(button) => {
                    let (x, y) = self.last_mouse.unwrap_or((0.0, 0.0));
                    let delay_ms = self.last_event
                        .map(|t| now.duration_since(t).as_millis().min(60_000) as u64)
                        .unwrap_or(0);
                    self.actions.push(Action::MouseButton { button: button_name(button).into(), down: true, x, y, delay_ms });
                    self.last_event = Some(now);
                }
                EventType::ButtonRelease(button) => {
                    let (x, y) = self.last_mouse.unwrap_or((0.0, 0.0));
                    let delay_ms = self.last_event
                        .map(|t| now.duration_since(t).as_millis().min(60_000) as u64)
                        .unwrap_or(0);
                    self.actions.push(Action::MouseButton { button: button_name(button).into(), down: false, x, y, delay_ms });
                    self.last_event = Some(now);
                }
                EventType::KeyPress(key) => {
                    let delay_ms = self.last_event
                        .map(|t| now.duration_since(t).as_millis().min(60_000) as u64)
                        .unwrap_or(0);
                    self.actions.push(Action::Key { key: format!("{key:?}"), down: true, text: event.name, delay_ms });
                    self.last_event = Some(now);
                }
                EventType::KeyRelease(key) => {
                    let delay_ms = self.last_event
                        .map(|t| now.duration_since(t).as_millis().min(60_000) as u64)
                        .unwrap_or(0);
                    self.actions.push(Action::Key { key: format!("{key:?}"), down: false, text: event.name, delay_ms });
                    self.last_event = Some(now);
                }
                EventType::Wheel { .. } => {}
            }
        }
    }

    fn play(&mut self) {
        if self.recording || self.playing || self.actions.is_empty() { return; }
        let actions = self.actions.clone();
        let repeats = self.repeat_count.max(1);
        let repeat_forever = self.repeat_forever;
        // Each multiplier scales recorded delays. Lower values play faster.
        let speed_multiplier = [1.0_f64, 0.85, 0.65, 0.40, 0.20][self.speed_index.min(4)];
        let stop = self.playback_stop.clone();
        stop.store(false, Ordering::SeqCst);
        self.playing = true;
        self.status = "Playing macro…".into();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let mut enigo = match Enigo::new(&Settings::default()) {
                Ok(e) => e,
                Err(e) => {
                    let _ = tx.send(RecorderMessage::PlaybackFinished(format!("Could not initialize input playback: {e}")));
                    return;
                }
            };
            let mut outcome = "Playback complete".to_string();
            let mut completed_repeats = 0usize;
            'playback: loop {
                if !repeat_forever && completed_repeats >= repeats { break; }
                for action in &actions {
                    if stop.load(Ordering::SeqCst) {
                        outcome = "Playback stopped".to_string();
                        break 'playback;
                    }
                    let delay = match action { Action::MouseMove { delay_ms, .. } | Action::MouseButton { delay_ms, .. } | Action::Key { delay_ms, .. } => *delay_ms };
                    // Sleep in short intervals so Stop playback responds promptly even during long delays.
                    let scaled_delay = ((delay.min(60_000) as f64) * speed_multiplier).round() as u64;
                    let deadline = Instant::now() + Duration::from_millis(scaled_delay);
                    while Instant::now() < deadline {
                        if stop.load(Ordering::SeqCst) {
                            outcome = "Playback stopped".to_string();
                            break 'playback;
                        }
                        thread::sleep(Duration::from_millis(1).min(deadline.saturating_duration_since(Instant::now())));
                    }
                    let result = match action {
                        Action::MouseMove { x, y, .. } => enigo.move_mouse(*x as i32, *y as i32, Coordinate::Abs),
                        Action::MouseButton { button, down, x, y, .. } => {
                            let move_result = enigo.move_mouse(*x as i32, *y as i32, Coordinate::Abs);
                            if move_result.is_err() { move_result } else if let Some(b) = parse_button(button) {
                                enigo.button(b, if *down { Direction::Press } else { Direction::Release })
                            } else { Ok(()) }
                        }
                        Action::Key { key, down, text, .. } => {
                            if let Some(k) = parse_key(key, text.as_deref()) {
                                enigo.key(k, if *down { Direction::Press } else { Direction::Release })
                            } else { Ok(()) }
                        }
                    };
                    if let Err(error) = result {
                        outcome = format!("Playback input error: {error}");
                        break 'playback;
                    }
                }
                completed_repeats += 1;
            }
            let _ = tx.send(RecorderMessage::PlaybackFinished(outcome));
        });
    }

    fn save(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("MacroLite macro", &["json"]).set_file_name("macro.json").save_file() {
            match serde_json::to_string_pretty(&self.actions).and_then(|data| fs::write(&path, data).map_err(serde_json::Error::io)) {
                Ok(()) => { self.macro_path = Some(path); self.status = "Macro saved".into(); }
                Err(e) => self.status = format!("Save failed: {e}"),
            }
        }
    }

    fn load(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("MacroLite macro", &["json"]).pick_file() {
            match fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<Vec<Action>>(&s).map_err(|e| e.to_string())) {
                Ok(actions) => { self.actions = actions; self.macro_path = Some(path); self.status = format!("Loaded {} actions", self.actions.len()); }
                Err(e) => self.status = format!("Load failed: {e}"),
            }
        }
    }
}

impl eframe::App for MacroLite {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_events();
        ctx.request_repaint_after(Duration::from_millis(30));

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.heading(egui::RichText::new("MacroLite").size(25.0).strong());
                ui.add_space(8.0);
                ui.label(egui::RichText::new("RECORD  •  REPLAY  •  REPEAT").small().color(egui::Color32::from_rgb(100, 190, 220)));
            });
            ui.add_space(10.0);

            egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                ui.label(egui::RichText::new("CONTROLS").small().strong().color(egui::Color32::from_rgb(100, 190, 220)));
                ui.add_space(7.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(!self.recording && !self.playing, egui::Button::new(egui::RichText::new("●  Record").color(egui::Color32::from_rgb(255, 125, 125))).min_size([105.0, 36.0].into())).clicked() { self.start_recording(); }
                    if ui.add_enabled(self.recording, egui::Button::new("■  Stop").min_size([82.0, 36.0].into())).clicked() { self.stop_recording(); }
                    if ui.add_enabled(!self.recording && !self.playing && !self.actions.is_empty(), egui::Button::new(egui::RichText::new("▶  Play").color(egui::Color32::from_rgb(105, 220, 165))).min_size([82.0, 36.0].into())).clicked() { self.play(); }
                    if ui.add_enabled(self.playing, egui::Button::new("■  Stop playback").min_size([115.0, 36.0].into())).clicked() { self.playback_stop.store(true, Ordering::SeqCst); self.status = "Stopping playback…".into(); }
                });
                ui.add_space(9.0);
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut self.repeat_forever, "Repeat infinitely");
                    ui.add_enabled_ui(!self.repeat_forever, |ui| {
                        ui.label("Repeats");
                        ui.add(egui::DragValue::new(&mut self.repeat_count).range(1..=100).speed(1));
                    });
                    ui.separator();
                    if ui.add_enabled(!self.recording && !self.playing, egui::Button::new("Save")).clicked() { self.save(); }
                    if ui.add_enabled(!self.recording && !self.playing, egui::Button::new("Load")).clicked() { self.load(); }
                    if ui.add_enabled(!self.recording && !self.playing, egui::Button::new("Clear")).clicked() { self.actions.clear(); self.status = "Macro cleared".into(); }
                });
                ui.add_space(7.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Playback speed");
                    let speeds = ["Identical to recording", "Slightly faster", "Faster", "Much faster", "Much, much faster"];
                    egui::ComboBox::from_id_source("playback_speed")
                        .selected_text(speeds[self.speed_index.min(4)])
                        .width(210.0)
                        .show_ui(ui, |ui| {
                            for (index, label) in speeds.iter().enumerate() {
                                ui.selectable_value(&mut self.speed_index, index, *label);
                            }
                        });
                });
            });

            ui.add_space(9.0);
            egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let (label, color) = if self.recording {
                        ("RECORDING", egui::Color32::from_rgb(255, 105, 105))
                    } else if self.playing {
                        ("PLAYING", egui::Color32::from_rgb(90, 220, 155))
                    } else {
                        ("READY", egui::Color32::from_rgb(100, 190, 220))
                    };
                    ui.label(egui::RichText::new(format!("● {label}")).strong().color(color));
                    ui.separator();
                    ui.label(&self.status);
                    ui.separator();
                    ui.label(format!("{} actions", self.actions.len()));
                });
                if let Some(path) = &self.macro_path {
                    ui.add_space(3.0);
                    ui.label(egui::RichText::new(path.display().to_string()).small().color(egui::Color32::from_rgb(145, 155, 170)));
                }
            });

            ui.add_space(8.0);
            egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(10.0)).show(ui, |ui| {
                ui.label(egui::RichText::new("CUSTOM HOTKEYS").small().strong().color(egui::Color32::from_rgb(100, 190, 220)));
                ui.add_space(5.0);
                let capture_active = self.capture_hotkey.lock().map(|v| *v).unwrap_or(None);
                let rows = [
                    (Hotkey::StartRecording, "Start recording"),
                    (Hotkey::StopRecording, "Stop recording"),
                    (Hotkey::StartPlayback, "Start playback"),
                    (Hotkey::StopPlayback, "Stop playback"),
                ];
                for (hotkey, label) in rows {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        ui.add_space(8.0);
                        let shortcut_text = self.hotkeys.lock().map(|config| config.get(hotkey).display()).unwrap_or_else(|_| "Unavailable".into());
                        ui.label(egui::RichText::new(shortcut_text).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let button_label = if capture_active == Some(hotkey) { "Press a key combo…" } else { "Set" };
                            if ui.button(button_label).clicked() {
                                if let Ok(mut target) = self.capture_hotkey.lock() { *target = Some(hotkey); }
                                self.status = format!("Set shortcut for {label}: press the desired key combination");
                            }
                        });
                    });
                }
                ui.label(egui::RichText::new("Click Set, then press a key combination. Use at least one modifier for best results.").small().color(egui::Color32::GRAY));
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Support MacroLite").small().color(egui::Color32::from_rgb(160, 170, 185)));
                if ui.add(egui::Button::new(egui::RichText::new("♥ Donate via PayPal").color(egui::Color32::from_rgb(120, 205, 240)))).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab("https://www.paypal.com/donate/?cmd=_s-xclick&hosted_button_id=PVD2ZC47A5HBE"));
                }
            });
            ui.add_space(5.0);
            egui::Frame::none().inner_margin(egui::Margin::symmetric(3.0, 2.0)).show(ui, |ui| {
                ui.label(egui::RichText::new("QUICK GUIDE").small().strong().color(egui::Color32::from_rgb(100, 190, 220)));
                ui.add_space(3.0);
                ui.label("Record captures mouse movement, clicks and keyboard input. Mouse wheel scrolling is not supported yet.");
                ui.label("Start recording, switch to the target app, perform your actions, then stop recording.");
                ui.label("Speed adjusts recorded pauses. Identical to recording uses the captured timing; faster modes shorten pauses.");
            });
        });
    }
}

fn button_name(button: Button) -> &'static str { match button { Button::Left => "left", Button::Right => "right", Button::Middle => "middle", Button::Unknown(_) => "unknown" } }
fn parse_button(button: &str) -> Option<EButton> { match button { "left" => Some(EButton::Left), "right" => Some(EButton::Right), "middle" => Some(EButton::Middle), _ => None } }

fn parse_key(debug: &str, text: Option<&str>) -> Option<EKey> {
    use EKey::*;
    Some(match debug {
        "Alt" => Alt, "AltGr" => Alt, "Backspace" => Backspace, "CapsLock" => CapsLock,
        "ControlLeft" => LControl, "ControlRight" => RControl, "Delete" => Delete,
        "DownArrow" => DownArrow, "End" => End, "Escape" => Escape, "F1" => F1, "F2" => F2,
        "F3" => F3, "F4" => F4, "F5" => F5, "F6" => F6, "F7" => F7, "F8" => F8,
        "F9" => F9, "F10" => F10, "F11" => F11, "F12" => F12, "Home" => Home,
        "LeftArrow" => LeftArrow, "MetaLeft" => Meta, "MetaRight" => Meta, "PageDown" => PageDown,
        "PageUp" => PageUp, "Return" => Return, "RightArrow" => RightArrow, "ShiftLeft" => LShift,
        "ShiftRight" => RShift, "Space" => Space, "Tab" => Tab, "UpArrow" => UpArrow,
        _ => {
            if let Some(s) = text {
                let mut chars = s.chars();
                if let (Some(c), std::option::Option::None) = (chars.next(), chars.next()) {
                    Unicode(c)
                } else {
                    return std::option::Option::None;
                }
            } else {
                return std::option::Option::None;
            }
        }
    })
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([650.0, 460.0])
            .with_min_inner_size([580.0, 410.0]),
        ..Default::default()
    };
    eframe::run_native("MacroLite", options, Box::new(|cc| {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(20, 24, 32);
        visuals.window_fill = egui::Color32::from_rgb(25, 30, 40);
        visuals.faint_bg_color = egui::Color32::from_rgb(31, 38, 49);
        visuals.extreme_bg_color = egui::Color32::from_rgb(14, 17, 23);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(31, 38, 49);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(38, 47, 60);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(48, 65, 82);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(37, 111, 139);
        visuals.selection.bg_fill = egui::Color32::from_rgb(35, 115, 145);
        visuals.selection.stroke.color = egui::Color32::from_rgb(170, 225, 240);
        cc.egui_ctx.set_visuals(visuals);
        Ok(Box::<MacroLite>::default())
    }))
}
