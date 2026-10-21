//! What the launcher does outside its window: where things are, the disc
//! images it finds, and the commands it runs (git, this repository's scripts,
//! the game), whose output comes back to the window line by line.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// This repository: the launcher sets the game up with its scripts, as a git
/// clone it keeps up to date in the data folder.
pub const REPOSITORY: &str = "https://github.com/Ken5998/rusty-mcla-arm64";
pub const PROJECT_PAGE: &str = "https://github.com/Ken5998/rusty-mcla-arm64";

/// Free space an install needs: the game data built from the disc (13.7 GB,
/// upstream's figure) with room to make it, the game and ffmpeg (the disc
/// image stays where it is).
pub const INSTALL_SPACE: u64 = 16_000_000_000;

/// Everything lives in one folder, the one scripts/install.sh uses by default,
/// so a game set up from the command line is found too.
#[derive(Clone)]
pub struct Paths {
    pub data: PathBuf,
}

impl Paths {
    pub fn new() -> Self {
        let share = std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"));
        Self { data: share.join("RustyMCLA-arm64") }
    }
    pub fn overlay(&self) -> PathBuf {
        self.data.join("overlay")
    }
    pub fn dist(&self) -> PathBuf {
        self.data.join("game")
    }
    pub fn game(&self) -> PathBuf {
        self.dist().join("mcla.sh")
    }
    pub fn built(&self) -> bool {
        self.game().is_file()
    }
    /// The disc image install.sh set before, which an update keeps when none
    /// is chosen.
    pub fn has_disc(&self) -> bool {
        self.data.join("disc.txt").is_file()
    }
    /// The release of this repository whose game build is installed, if any.
    pub fn release(&self) -> Option<String> {
        let tag = fs::read_to_string(self.data.join("build/release")).ok()?;
        let tag = tag.trim();
        (!tag.is_empty()).then(|| tag.to_string())
    }
    pub fn log(&self) -> PathBuf {
        self.data.join("launcher.log")
    }
    fn settings(&self) -> PathBuf {
        self.data.join("launcher.conf")
    }
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// The game's window sizes the launcher offers: 1280x720 is the original's
/// own frame and the lightest; Steam's Game Mode scales the window to the screen.
pub const RESOLUTIONS: &[(&str, &str)] = &[
    ("1280x720", "The Xbox 360's own resolution: the most frames per second"),
    ("1600x900", "Sharper, more power"),
    ("1920x1080", "The Retroid Pocket 6's screen: the sharpest, the most power"),
];

/// What the window remembers between runs.
#[derive(Clone, PartialEq)]
pub struct Settings {
    pub disc: String,
    /// The game's window, "1280x720" and the like.
    pub resolution: String,
    /// Borderless full screen instead of a window of that size.
    pub fill: bool,
    /// The frame rate cap: 60, 30 or 0 (none).
    pub fps_cap: u32,
    pub add_to_steam: bool,
}

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        let mut s = Settings {
            disc: String::new(),
            resolution: RESOLUTIONS[0].0.to_string(),
            fill: false,
            fps_cap: 60,
            add_to_steam: true,
        };
        for line in fs::read_to_string(paths.settings()).unwrap_or_default().lines() {
            match line.split_once('=') {
                Some(("disc", v)) => s.disc = v.to_string(),
                Some(("resolution", v)) if RESOLUTIONS.iter().any(|(r, _)| *r == v) => s.resolution = v.to_string(),
                Some(("fill", v)) => s.fill = v == "1",
                Some(("fps_cap", v)) => s.fps_cap = v.parse().ok().filter(|n| [0, 30, 60].contains(n)).unwrap_or(60),
                Some(("add_to_steam", v)) => s.add_to_steam = v != "0",
                _ => {}
            }
        }
        s
    }
    pub fn save(&self, paths: &Paths) {
        let _ = fs::create_dir_all(&paths.data);
        let _ = fs::write(
            paths.settings(),
            format!(
                "disc={}\nresolution={}\nfill={}\nfps_cap={}\nadd_to_steam={}\n",
                self.disc,
                self.resolution,
                if self.fill { 1 } else { 0 },
                self.fps_cap,
                if self.add_to_steam { 1 } else { 0 }
            ),
        );
    }
    pub fn window_mode(&self) -> &'static str {
        if self.fill { "borderless" } else { "windowed" }
    }
}

/// Free bytes on the file system that holds `path` (or its closest parent).
pub fn free_space(path: &Path) -> Option<u64> {
    let mut p = path.to_path_buf();
    while !p.exists() {
        p = p.parent()?.to_path_buf();
    }
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0).then(|| st.f_bavail as u64 * st.f_frsize as u64)
}

pub fn gigabytes(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
}

/// Files in the usual places (the home folder, Downloads, ROM and emulation
/// folders, SD cards) that `wanted` accepts, the ones `first` likes first,
/// then the newest.
fn scan(wanted: impl Fn(&str, u64) -> bool, first: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let home = home();
    let mut roots: Vec<(PathBuf, u32)> = vec![(home.clone(), 1)];
    for sub in ["Downloads", "Desktop", "Documents", "Games", "ROMs", "roms", "Emulation"] {
        roots.push((home.join(sub), 3));
    }
    if let Ok(user) = std::env::var("USER") {
        if let Ok(cards) = fs::read_dir(Path::new("/run/media").join(user)) {
            roots.extend(cards.flatten().map(|c| (c.path(), 3)));
        }
    }
    let mut found: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    fn walk(
        dir: &Path,
        depth: u32,
        wanted: &dyn Fn(&str, u64) -> bool,
        found: &mut Vec<(PathBuf, std::time::SystemTime)>,
    ) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() && depth > 0 {
                walk(&e.path(), depth - 1, wanted, found);
            } else if kind.is_file() {
                if let Ok(m) = e.metadata() {
                    if wanted(&name, m.len()) {
                        found.push((e.path(), m.modified().unwrap_or(std::time::UNIX_EPOCH)));
                    }
                }
            }
        }
    }
    for (root, depth) in roots {
        walk(&root, depth, &wanted, &mut found);
    }
    found.sort_by_key(|(p, t)| {
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        (!first(&name), std::cmp::Reverse(*t))
    });
    found.dedup_by(|a, b| a.0 == b.0);
    found.into_iter().map(|(p, _)| p).collect()
}

/// Disc images (an Xbox 360 game disc is several GB), Midnight Club: Los
/// Angeles first.
pub fn scan_discs() -> Vec<PathBuf> {
    scan(|name, size| name.ends_with(".iso") && size > 1 << 30, is_mcla)
}

pub fn is_mcla(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("mcla")
        || (name.contains("midnight club") || name.contains("midnight_club") || name.contains("midnightclub"))
            && (name.contains("los angeles") || name.contains("la") && !name.contains("3") && !name.contains("dub"))
}

/// What a running task sends to the window.
pub enum Event {
    Line(String),
    Finished(Result<String, String>),
}

/// Runs one task at a time in the background and stops it on request.
pub struct Runner {
    pub tx: Sender<Event>,
    pub cancel: Arc<AtomicBool>,
    /// Process group of the command that runs now (0 = none).
    group: Arc<AtomicI32>,
    log: Arc<Mutex<Option<fs::File>>>,
    /// The last "error: ..." line the running command printed, which says
    /// what went wrong better than its exit status.
    error: Arc<Mutex<Option<String>>>,
}

impl Runner {
    pub fn new(tx: Sender<Event>) -> Self {
        Self {
            tx,
            cancel: Arc::new(AtomicBool::new(false)),
            group: Arc::new(AtomicI32::new(0)),
            log: Arc::new(Mutex::new(None)),
            error: Arc::new(Mutex::new(None)),
        }
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        let group = self.group.load(Ordering::Relaxed);
        if group > 0 {
            unsafe { libc::kill(-group, libc::SIGTERM) };
        }
    }

    fn clone_handle(&self) -> Runner {
        Runner {
            tx: self.tx.clone(),
            cancel: self.cancel.clone(),
            group: self.group.clone(),
            log: self.log.clone(),
            error: self.error.clone(),
        }
    }

    fn line(&self, text: impl Into<String>) {
        let text = text.into();
        if let Some(message) = text.strip_prefix("error: ") {
            *self.error.lock().unwrap() = Some(message.to_string());
        }
        if let Some(f) = self.log.lock().unwrap().as_mut() {
            let _ = writeln!(f, "{text}");
        }
        let _ = self.tx.send(Event::Line(text));
    }

    /// Runs `work` on a new thread with a fresh log file; its result ends the task.
    pub fn spawn(&self, log: PathBuf, work: impl FnOnce(&Runner) -> Result<String, String> + Send + 'static) {
        self.cancel.store(false, Ordering::Relaxed);
        if let Some(parent) = log.parent() {
            let _ = fs::create_dir_all(parent);
        }
        *self.log.lock().unwrap() = fs::File::create(&log).ok();
        let r = self.clone_handle();
        std::thread::spawn(move || {
            let result = work(&r);
            if let Err(e) = &result {
                r.line(format!("ERROR: {e}"));
            }
            *r.log.lock().unwrap() = None;
            let _ = r.tx.send(Event::Finished(result));
        });
    }

    /// Runs a command in its own process group, passing on every line it
    /// prints; Err when it fails or the task was cancelled.
    pub fn run(&self, cmd: &mut Command) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        self.line(format!("$ {}", describe(cmd)));
        *self.error.lock().unwrap() = None;
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", cmd.get_program().to_string_lossy()))?;
        self.group.store(child.id() as i32, Ordering::Relaxed);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let mut readers = 0;
        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
                       child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
            .into_iter()
            .flatten()
        {
            let r = self.clone_handle();
            let done = done_tx.clone();
            std::thread::spawn(move || {
                forward_lines(BufReader::new(stream), &r);
                let _ = done.send(());
            });
            readers += 1;
        }
        let status = child.wait().map_err(|e| e.to_string());
        // The last lines, but not for long: a program the command started in
        // the background may keep its output open.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        for _ in 0..readers {
            let left = until.saturating_duration_since(std::time::Instant::now());
            if done_rx.recv_timeout(left).is_err() {
                break;
            }
        }
        self.group.store(0, Ordering::Relaxed);
        if self.cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        match status? {
            s if s.success() => Ok(()),
            s => Err(self.error.lock().unwrap().take().map(|e| capitalize(&e)).unwrap_or_else(|| {
                format!("{} failed ({s})", cmd.get_program().to_string_lossy())
            })),
        }
    }
}

/// Passes on each line a command prints. Progress lines end in \r (curl on
/// a terminal), so those end a line too.
fn forward_lines(reader: impl BufRead, r: &Runner) {
    let mut line = Vec::new();
    let flush = |line: &mut Vec<u8>| {
        if !line.is_empty() {
            r.line(String::from_utf8_lossy(line).into_owned());
            line.clear();
        }
    };
    for byte in reader.bytes() {
        match byte {
            Ok(b'\n' | b'\r') => flush(&mut line),
            Ok(b) => line.push(b),
            Err(_) => break,
        }
    }
    flush(&mut line);
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

fn describe(cmd: &Command) -> String {
    std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Clones this repository into the data folder, or brings the clone up to
/// date with main. Offline, an existing clone is used as it is.
pub fn update_overlay(r: &Runner, paths: &Paths) -> Result<(), String> {
    let overlay = paths.overlay();
    if overlay.join(".git").is_dir() {
        let fetched = r.run(Command::new("git").arg("-C").arg(&overlay).args(["fetch", "--depth", "50", "origin", "main"]));
        match fetched {
            Ok(()) => r.run(Command::new("git").arg("-C").arg(&overlay).args(["reset", "--hard", "-q", "FETCH_HEAD"]))?,
            Err(e) if e == "Cancelled" => return Err(e),
            Err(_) => r.line("Could not reach GitHub: going on with the scripts already here."),
        }
    } else {
        fs::create_dir_all(&paths.data).map_err(|e| e.to_string())?;
        let _ = fs::remove_dir_all(&overlay);
        r.run(Command::new("git").args(["clone", "-q", "--depth", "50", REPOSITORY]).arg(&overlay))?;
    }
    let head = Command::new("git").arg("-C").arg(&overlay).args(["log", "-1", "--format=%h %s"]).output();
    if let Ok(out) = head {
        r.line(format!("Scripts and patches: {}", String::from_utf8_lossy(&out.stdout).trim()));
    }
    Ok(())
}

/// The whole install: scripts/install.sh with the disc image, or "-" to keep
/// the one set before.
pub fn install(r: &Runner, paths: &Paths, disc: Option<PathBuf>, settings: &Settings) -> Result<String, String> {
    update_overlay(r, paths)?;
    r.run(
        Command::new("bash")
            .arg(paths.overlay().join("scripts/install.sh"))
            .arg(disc.map(|p| p.into_os_string()).unwrap_or_else(|| "-".into()))
            .arg(&paths.data)
            .env("MCLA_RESOLUTION", &settings.resolution)
            .env("MCLA_WINDOW_MODE", settings.window_mode())
            .env("MCLA_FPS_CAP", settings.fps_cap.to_string())
            .env("MCLA_ADD_TO_STEAM", if settings.add_to_steam { "1" } else { "0" }),
    )?;
    Ok(if settings.add_to_steam {
        "Midnight Club: Los Angeles is ready. Start it from Steam for the handheld's controls.".into()
    } else {
        "Midnight Club: Los Angeles is ready.".into()
    })
}

pub fn add_to_steam(r: &Runner, paths: &Paths) -> Result<String, String> {
    if !paths.overlay().join("scripts/add-to-steam.sh").is_file() {
        update_overlay(r, paths)?;
    }
    r.run(Command::new("bash").arg(paths.overlay().join("scripts/add-to-steam.sh")).arg(&paths.data))?;
    Ok("Midnight Club: Los Angeles is in your Steam library.".into())
}

/// Starts the game on its own, so it keeps running when the launcher closes.
pub fn play(paths: &Paths, args: &[&str]) -> Result<(), String> {
    Command::new(paths.game())
        .args(args)
        .current_dir(paths.dist())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start the game: {e}"))
}

pub fn open(url: &str) {
    let _ = Command::new("xdg-open").arg(url).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// The kernel option that stops MangoHud and the Steam overlay from
/// flickering over the game on Adreno GPUs: with GPU preemption on, the msm
/// driver switches to gamescope's composition in the middle of the game's
/// frame and what gamescope draws comes out garbled.
pub const PREEMPTION_OFF: &str = "msm.enable_preemption=0";

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Preemption {
    /// Not an Adreno GPU (no msm driver) or no rpm-ostree: nothing to offer.
    Unavailable,
    /// The running kernel has the option.
    Off,
    /// Set for the next boot.
    OffAfterRestart,
    /// Still on: the overlays can flicker.
    On,
}

pub fn preemption() -> Preemption {
    if !Path::new("/sys/module/msm").exists() {
        return Preemption::Unavailable;
    }
    let running = fs::read_to_string("/proc/cmdline").unwrap_or_default();
    if has_argument(&running, PREEMPTION_OFF) {
        return Preemption::Off;
    }
    // The options of the next boot, which reading needs no password for.
    match Command::new("rpm-ostree").arg("kargs").stdin(Stdio::null()).stderr(Stdio::null()).output() {
        Ok(out) if out.status.success() => {
            if has_argument(&String::from_utf8_lossy(&out.stdout), PREEMPTION_OFF) {
                Preemption::OffAfterRestart
            } else {
                Preemption::On
            }
        }
        _ => Preemption::Unavailable,
    }
}

fn has_argument(line: &str, argument: &str) -> bool {
    line.split_whitespace().any(|a| a == argument)
}

/// Adds the option for the next boots. It changes the system, so pkexec asks
/// for the user's password (the one sudo takes) first.
pub fn turn_off_preemption(r: &Runner) -> Result<String, String> {
    r.run(Command::new("pkexec").args(["rpm-ostree", "kargs", &format!("--append-if-missing={PREEMPTION_OFF}")]))
        .map_err(|e| format!("{e} (no password given?)"))?;
    Ok("GPU preemption turns off at the next start: restart the handheld.".into())
}

#[cfg(test)]
mod tests {
    use super::{PREEMPTION_OFF, has_argument, is_mcla};

    #[test]
    fn knows_the_game_by_its_names() {
        assert!(is_mcla("Midnight Club - Los Angeles - Complete Edition (USA, Europe).iso"));
        assert!(is_mcla("midnight_club_la.iso"));
        assert!(is_mcla("MCLA.iso"));
        assert!(!is_mcla("Midnight Club 3 - DUB Edition Remix (USA).iso"));
        assert!(!is_mcla("Forza Motorsport 2.iso"));
    }

    #[test]
    fn finds_the_kernel_option() {
        assert!(has_argument("BOOT_IMAGE=/vmlinuz rw msm.enable_preemption=0 quiet", PREEMPTION_OFF));
        assert!(!has_argument("rw msm.enable_preemption=1 quiet", PREEMPTION_OFF));
        assert!(!has_argument("rw xmsm.enable_preemption=0", PREEMPTION_OFF));
    }
}
