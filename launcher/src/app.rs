//! The launcher window.

use crate::tasks::{self, Event, Paths, Preemption, Runner, Settings};
use eframe::egui::{self, Color32, RichText};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant, SystemTime};

const ERROR: Color32 = Color32::from_rgb(230, 110, 90);
const GOOD: Color32 = Color32::from_rgb(120, 200, 120);
const LOG_LINES: usize = 4000;

#[derive(PartialEq, Clone, Copy)]
enum Task {
    Install,
    Steam,
    Preemption,
}

pub struct LauncherApp {
    paths: Paths,
    settings: Settings,
    discs: Vec<PathBuf>,
    free: Option<u64>,
    runner: Runner,
    rx: Receiver<Event>,
    running: Option<Task>,
    started: Instant,
    step: String,
    progress: Option<f32>,
    log: VecDeque<String>,
    result: Option<Result<String, String>>,
    show_log: bool,
    error: Option<String>,
    preemption: Preemption,
    /// The settings the game was last set up with: a change shows that
    /// Update applies it.
    installed: Settings,
}

impl LauncherApp {
    pub fn new(_ctx: egui::Context) -> Self {
        let paths = Paths::new();
        let mut settings = Settings::load(&paths);
        let discs = tasks::scan_discs();
        // The disc image used last time, or the first one found.
        if !PathBuf::from(&settings.disc).is_file() && !paths.has_disc() {
            settings.disc = discs.first().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        }
        let (tx, rx) = channel();
        let free = tasks::free_space(&paths.data);
        Self {
            installed: settings.clone(),
            paths,
            settings,
            discs,
            free,
            runner: Runner::new(tx),
            rx,
            running: None,
            started: Instant::now(),
            step: String::new(),
            progress: None,
            log: VecDeque::new(),
            result: None,
            show_log: false,
            error: None,
            preemption: tasks::preemption(),
        }
    }

    fn chosen(text: &str) -> Option<PathBuf> {
        let text = text.trim().trim_matches('"');
        (!text.is_empty()).then(|| PathBuf::from(text))
    }

    fn start(&mut self, task: Task) {
        let disc = Self::chosen(&self.settings.disc);
        if task == Task::Install {
            match &disc {
                Some(p) if !p.is_file() => {
                    self.error = Some(format!("{} is not a file.", p.display()));
                    return;
                }
                None if !self.paths.has_disc() => {
                    self.error = Some("Choose your Midnight Club: Los Angeles disc image first.".into());
                    return;
                }
                _ => {}
            }
        }
        self.settings.save(&self.paths);
        self.error = None;
        self.result = None;
        self.log.clear();
        self.step = match task {
            Task::Install => "Getting the latest scripts".into(),
            Task::Steam => "Adding Midnight Club: Los Angeles to Steam".into(),
            Task::Preemption => "Turning GPU preemption off (asks for your password)".into(),
        };
        self.progress = None;
        self.running = Some(task);
        self.started = Instant::now();
        let paths = self.paths.clone();
        let settings = self.settings.clone();
        let log = paths.log();
        match task {
            Task::Install => {
                self.installed = settings.clone();
                self.runner.spawn(log, move |r| tasks::install(r, &paths, disc, &settings))
            }
            Task::Steam => self.runner.spawn(log, move |r| tasks::add_to_steam(r, &paths)),
            Task::Preemption => self.runner.spawn(log, tasks::turn_off_preemption),
        }
    }

    /// Takes in what the running task printed.
    fn pump(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Line(line) => {
                    if let Some(step) = line.strip_prefix("==> ") {
                        self.step = step.to_string();
                        self.progress = None;
                    } else if let Some((stage, p)) = install_progress(&line) {
                        self.step = format!("Building the game data from your disc: {}", stage_name(&stage));
                        self.progress = Some(p);
                    } else if let Some(p) = curl_progress(&line) {
                        self.progress = Some(p);
                    }
                    self.log.push_back(line);
                    while self.log.len() > LOG_LINES {
                        self.log.pop_front();
                    }
                }
                Event::Finished(result) => {
                    self.running = None;
                    self.progress = None;
                    if matches!(&result, Err(e) if e != "Cancelled") {
                        self.show_log = true;
                    }
                    self.result = Some(result);
                    self.free = tasks::free_space(&self.paths.data);
                    self.preemption = tasks::preemption();
                }
            }
        }
    }

    fn game_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).inner_margin(12.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if self.paths.built() {
                let when = std::fs::metadata(self.paths.game())
                    .and_then(|m| m.modified())
                    .map(local_time)
                    .unwrap_or_default();
                let release = self.paths.release().unwrap_or_default();
                ui.label(
                    RichText::new(format!("✔ Ready to play (game build {release}, set up {when})")).color(GOOD).strong(),
                );
                ui.label(
                    RichText::new(
                        "Start it from Steam (\"Midnight Club: Los Angeles\" in the library) so the handheld's \
                         controls reach it. Update gets the newest build of the game and applies the settings \
                         below; the game data stays.",
                    )
                    .weak(),
                );
            } else {
                ui.label(RichText::new("○ Not set up yet").strong());
                ui.label(
                    RichText::new(
                        "Install downloads the ARM64 build of Rusty MCLA and ffmpeg (about 200 MB), then builds \
                         the game data from your disc (about 14 GB). Keep the handheld on the charger.",
                    )
                    .weak(),
                );
            }
        });
    }

    /// GPU preemption on Adreno: MangoHud and the Steam overlay flicker over
    /// the game until it is off (a kernel option, so for every game).
    fn preemption_row(&mut self, ui: &mut egui::Ui) {
        match self.preemption {
            Preemption::Unavailable => {}
            Preemption::Off => {
                ui.label(RichText::new("✔ GPU preemption is off: MangoHud and the Steam overlay draw cleanly.").weak());
            }
            Preemption::OffAfterRestart => {
                ui.label(
                    RichText::new("GPU preemption turns off at the next start: restart the handheld.").color(GOOD),
                );
            }
            Preemption::On => {
                ui.horizontal_wrapped(|ui| {
                    ui.label("MangoHud and the Steam overlay flicker over the game while GPU preemption is on.");
                    if ui
                        .add_enabled(self.running.is_none(), egui::Button::new("Turn it off"))
                        .on_hover_text(format!(
                            "Adds the kernel option {} (asks for your password, then restart the handheld). \
                             Undo: sudo rpm-ostree kargs --delete={}",
                            tasks::PREEMPTION_OFF,
                            tasks::PREEMPTION_OFF
                        ))
                        .clicked()
                    {
                        self.start(Task::Preemption);
                    }
                });
            }
        }
    }

    fn picker(&mut self, ui: &mut egui::Ui) {
        let label = "Your Xbox 360 disc image of Midnight Club: Los Angeles, Complete Edition";
        ui.label(if self.paths.has_disc() { format!("{label} (optional: the one set before stays)") } else { label.into() });
        let text = &mut self.settings.disc;
        ui.horizontal(|ui| {
            let width = ui.available_width() - 100.0;
            ui.add(egui::TextEdit::singleline(text).hint_text("Disc image: .iso").desired_width(width));
            if ui.button("Browse...").clicked() {
                let mut dialog = rfd::FileDialog::new()
                    .set_title("Select your Midnight Club: Los Angeles disc image")
                    .add_filter("Xbox 360 disc image", &["iso", "ISO"]);
                if let Some(dir) = Self::chosen(text).and_then(|p| p.parent().map(PathBuf::from)).filter(|d| d.is_dir())
                {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(p) = dialog.pick_file() {
                    *text = p.to_string_lossy().into_owned();
                }
            }
        });
        let others: Vec<String> =
            self.discs.iter().map(|p| p.to_string_lossy().into_owned()).filter(|p| p != text).collect();
        if !others.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Found:").weak());
                for p in others.iter().take(4) {
                    let name = PathBuf::from(p).file_name().unwrap_or_default().to_string_lossy().into_owned();
                    if ui.small_button(name).on_hover_text(p).clicked() {
                        *text = p.clone();
                    }
                }
            });
        }
    }

    fn options(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Resolution:");
            for (res, tip) in tasks::RESOLUTIONS {
                let name = if *res == tasks::RESOLUTIONS[0].0 { format!("{res} (default)") } else { res.to_string() };
                ui.radio_value(&mut self.settings.resolution, res.to_string(), name).on_hover_text(*tip);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Frame rate cap:");
            ui.radio_value(&mut self.settings.fps_cap, 60, "60");
            ui.radio_value(&mut self.settings.fps_cap, 30, "30").on_hover_text("Steadier and lighter on the battery");
            ui.radio_value(&mut self.settings.fps_cap, 0, "None").on_hover_text("The screen's refresh still limits it with vsync");
        });
        ui.checkbox(&mut self.settings.fill, "Borderless full screen at the screen's resolution in Desktop Mode")
            .on_hover_text("Off: a window of the size above, which Steam's Game Mode scales to the screen");
        ui.checkbox(&mut self.settings.add_to_steam, "Add to Steam when the install is done");
        let installed = &self.installed;
        if self.paths.built()
            && (installed.resolution != self.settings.resolution
                || installed.fill != self.settings.fill
                || installed.fps_cap != self.settings.fps_cap)
        {
            ui.label(RichText::new("Press Update to apply the changed settings.").color(GOOD));
        }
    }

    fn buttons(&mut self, ui: &mut egui::Ui) {
        let built = self.paths.built();
        ui.horizontal_wrapped(|ui| {
            let big = |text: &str| egui::Button::new(RichText::new(text).strong().size(16.0)).min_size([130.0, 36.0].into());
            if self.running.is_some() {
                if ui.add(big("Cancel")).clicked() {
                    self.runner.stop();
                    self.step = "Cancelling...".into();
                }
            } else {
                if ui
                    .add(big(if built { "Update" } else { "Install" }))
                    .on_hover_text("Gets the newest game build and the latest scripts, checks the game data, then writes the settings")
                    .clicked()
                {
                    self.start(Task::Install);
                }
                if ui
                    .add_enabled(built, big("Play"))
                    .on_hover_text("Starts the game here; from Steam the handheld's controls work")
                    .clicked()
                {
                    if let Err(e) = tasks::play(&self.paths, &[]) {
                        self.error = Some(e);
                    }
                }
                if ui
                    .add_enabled(built, big("Add to Steam"))
                    .on_hover_text("Adds the game to the Steam library (Steam restarts)")
                    .clicked()
                {
                    self.start(Task::Steam);
                }
            }
            ui.toggle_value(&mut self.show_log, "Details");
            if ui.small_button("Project page").clicked() {
                tasks::open(tasks::PROJECT_PAGE);
            }
        });
    }

    fn status(&mut self, ui: &mut egui::Ui) {
        if self.running.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&self.step).strong());
                ui.label(RichText::new(elapsed(self.started.elapsed())).weak());
            });
            let bar = match self.progress {
                Some(p) => egui::ProgressBar::new(p).show_percentage(),
                None => egui::ProgressBar::new(0.0).animate(true),
            };
            ui.add(bar);
        }
        match &self.result {
            Some(Ok(done)) => {
                ui.label(RichText::new(done).color(GOOD).strong());
            }
            Some(Err(e)) if e == "Cancelled" => {
                ui.label("Cancelled. Press the button again to go on.");
            }
            Some(Err(e)) => {
                ui.colored_label(ERROR, e);
                ui.label(
                    RichText::new(format!(
                        "Fix the problem and press the button again. The full log is {}.",
                        self.paths.log().display()
                    ))
                    .weak(),
                );
            }
            None => {}
        }
        if let Some(e) = &self.error {
            ui.colored_label(ERROR, e);
        }
    }
}

impl eframe::App for LauncherApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();
        let ctx = ui.ctx().clone();
        if self.running.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("Midnight Club: Los Angeles").size(28.0).strong());
                    ui.label(RichText::new("for ARM64 handhelds").weak());
                });
                ui.label(
                    RichText::new(
                        "Plays Midnight Club: Los Angeles with Rusty MCLA, the unofficial PC port written in Rust, \
                         natively on ARM64 from your own Xbox 360 disc, and adds it to Steam. Nothing from the game \
                         is downloaded.",
                    )
                    .weak(),
                );
                ui.add_space(10.0);
                self.game_panel(ui);
                self.preemption_row(ui);
                ui.add_space(10.0);
                ui.add_enabled_ui(self.running.is_none(), |ui| {
                    self.picker(ui);
                    ui.add_space(6.0);
                    if let Some(free) = self.free.filter(|free| *free < tasks::INSTALL_SPACE && !self.paths.built()) {
                        ui.colored_label(
                            ERROR,
                            format!(
                                "Only {} free in {}: the install needs about 16 GB.",
                                tasks::gigabytes(free),
                                self.paths.data.display()
                            ),
                        );
                    }
                    self.options(ui);
                });
                ui.add_space(10.0);
                self.buttons(ui);
                ui.add_space(8.0);
                self.status(ui);
                if self.show_log {
                    ui.add_space(6.0);
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("log").max_height(220.0).stick_to_bottom(true).show(
                            ui,
                            |ui| {
                                ui.set_width(ui.available_width());
                                for line in &self.log {
                                    ui.label(RichText::new(line).monospace().size(11.0));
                                }
                            },
                        );
                    });
                }
            });
        });
    }
}

impl Drop for LauncherApp {
    fn drop(&mut self) {
        // Closing the window stops an install; the next one goes on from there.
        self.runner.stop();
    }
}

/// The stage and the share done of the game data install, from mcla-tool's
/// progress lines: "[extract] 1200 / 7800 MB (15%)", "[audio] 40 / 900 (4%)".
fn install_progress(line: &str) -> Option<(String, f32)> {
    let rest = line.strip_prefix('[')?;
    let (stage, rest) = rest.split_once("] ")?;
    let percent = rest.strip_suffix("%)")?.rsplit_once('(')?.1;
    let percent: f32 = percent.parse().ok()?;
    (stage.chars().all(|c| c.is_ascii_lowercase()) && (0.0..=100.0).contains(&percent))
        .then(|| (stage.to_string(), percent / 100.0))
}

/// What a stage of mcla-tool's install does, in words.
fn stage_name(stage: &str) -> &str {
    match stage {
        "validate" => "checking the disc",
        "checksum" => "checking the disc's checksum",
        "extract" => "extracting the files",
        "shaders" => "translating the shaders",
        "audio" => "converting the audio",
        "video" => "converting the films",
        "verify" => "checking the files",
        other => other,
    }
}

/// The share of a download done, from curl's progress meter: lines that
/// start with the percentage, "  42  98.1M   42 41.3M ...".
fn curl_progress(line: &str) -> Option<f32> {
    let mut fields = line.split_whitespace();
    let percent: f32 = fields.next()?.parse().ok()?;
    let total = fields.next()?;
    // The size column ends in a unit or is a plain byte count.
    if !total.trim_end_matches(['k', 'M', 'G']).chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    (0.0..=100.0).contains(&percent).then_some(percent / 100.0)
}

fn elapsed(d: Duration) -> String {
    let minutes = d.as_secs() / 60;
    match minutes {
        0 => "under a minute so far".into(),
        1..60 => format!("{minutes} min so far"),
        _ => format!("{} h {:02} min so far", minutes / 60, minutes % 60),
    }
}

fn local_time(t: SystemTime) -> String {
    let Ok(since) = t.duration_since(SystemTime::UNIX_EPOCH) else { return String::new() };
    let secs = since.as_secs() as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return String::new();
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min)
}

#[cfg(test)]
mod tests {
    use super::{curl_progress, elapsed, install_progress};
    use std::time::Duration;

    #[test]
    fn reads_curl_progress() {
        assert_eq!(
            curl_progress(" 42 98.1M   42 41.3M    0     0  12.1M      0  0:00:08  0:00:03  0:00:05 12.1M"),
            Some(0.42)
        );
        assert_eq!(curl_progress("100  185  100   185    0     0  2509k      0 --:--:--"), Some(1.0));
        assert_eq!(curl_progress("  % Total    % Received % Xferd  Average Speed"), None);
        assert_eq!(curl_progress("==> Getting the game"), None);
        assert_eq!(curl_progress("2 files changed"), None);
    }

    #[test]
    fn reads_install_progress() {
        assert_eq!(install_progress("[extract] 1200 / 8000 MB (15%)"), Some(("extract".into(), 0.15)));
        assert_eq!(install_progress("[audio] 40 / 900 (4%)"), Some(("audio".into(), 0.04)));
        assert_eq!(install_progress("[extract] done"), None);
        assert_eq!(install_progress("  [validate] ok   size: fine"), None);
    }

    #[test]
    fn says_how_long() {
        assert_eq!(elapsed(Duration::from_secs(59)), "under a minute so far");
        assert_eq!(elapsed(Duration::from_secs(12 * 60 + 5)), "12 min so far");
        assert_eq!(elapsed(Duration::from_secs(65 * 60)), "1 h 05 min so far");
    }
}
