//! Input from NordicNeuroLab hardware: SyncBox (scanner trigger) and ResponseGrips.
//!
//! The NNL SyncBox passes the scanner's TTL pulse – and the ResponseGrip button presses – on to
//! the stimulus PC either as USB keyboard key presses (no driver needed) or as characters on a
//! serial/COM port. Both paths end up in [`InputRouter`], which turns raw keys/characters into
//! trigger and button events. A built-in scanner simulator produces triggers at the TR so
//! paradigms can be tested on any laptop.
//!
//! Serial characters are time-stamped in the reader thread the moment they arrive, so trigger
//! timing in serial mode does not depend on the display's frame rate.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, LazyLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::model::Hardware;

pub const BAUD_RATES: [u32; 5] = [57600, 115200, 9600, 19200, 38400]; // tried in this order by auto-detect
const SYNCBOX_HINTS: [&str; 5] = ["syncbox", "sync box", "nordicneurolab", "nordic neuro", "nnl"];

static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Seconds on a monotonic clock (like Python's `time.perf_counter`).
pub fn now() -> f64 {
    EPOCH.elapsed().as_secs_f64()
}

pub fn instant_secs(i: Instant) -> f64 {
    i.saturating_duration_since(*EPOCH).as_secs_f64()
}

/// Something that wakes the UI when input arrives from another thread.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

pub fn no_waker() -> Waker {
    Arc::new(|| {})
}

// =============================================================================
// Ports
// =============================================================================
#[derive(Clone, Debug, PartialEq)]
pub struct PortInfo {
    pub device: String,
    pub description: String,
    pub manufacturer: String,
    pub vid_pid: Option<(u16, u16)>,
    pub likely: bool,
}

/// All serial ports with their USB details; likely SyncBoxes first.
pub fn describe_ports() -> Vec<PortInfo> {
    let mut out = vec![];
    for p in serialport::available_ports().unwrap_or_default() {
        let (description, manufacturer, vid_pid, extra) = match &p.port_type {
            serialport::SerialPortType::UsbPort(u) => (
                u.product.clone().unwrap_or_else(|| "USB serial device".into()),
                u.manufacturer.clone().unwrap_or_default(),
                Some((u.vid, u.pid)),
                u.serial_number.clone().unwrap_or_default(),
            ),
            serialport::SerialPortType::BluetoothPort => ("Bluetooth".into(), String::new(), None, String::new()),
            serialport::SerialPortType::PciPort => ("PCI serial port".into(), String::new(), None, String::new()),
            serialport::SerialPortType::Unknown => ("serial port".into(), String::new(), None, String::new()),
        };
        let text = format!("{description} {manufacturer} {extra} {}", p.port_name).to_lowercase();
        let likely = SYNCBOX_HINTS.iter().any(|h| text.contains(h));
        out.push(PortInfo { device: p.port_name.clone(), description, manufacturer, vid_pid, likely });
    }
    out.sort_by(|a, b| (!a.likely, a.device.clone()).cmp(&(!b.likely, b.device.clone())));
    out
}

/// Turn a serial-port error into something an operator can act on.
/// Shown wherever NNL hardware is mentioned prominently (About, Hardware dialog, README, releases).
pub const NNL_DISCLAIMER: &str = "Designed for compatibility with NordicNeuroLab (NNL) hardware – the SyncBox and the \
    ResponseGrips. ParadigmShift is an independent project: it is not affiliated with, endorsed, sponsored or supported \
    by NordicNeuroLab AS. NordicNeuroLab, NNL, SyncBox and ResponseGrip are names of NordicNeuroLab AS, used here only \
    to describe compatibility.";

/// Where FTDI's Virtual COM Port driver can be downloaded.
pub const FTDI_VCP_URL: &str = "https://ftdichip.com/drivers/vcp-drivers/";

/// The SyncBox talks USB through an FTDI chip: without FTDI's VCP driver it does not show up as a
/// serial port at all.
pub const FTDI_DRIVER_NOTE: &str = "The SyncBox's USB connection uses an FTDI chip. In serial mode it only appears as a \
    serial (COM) port when FTDI's VCP driver (Virtual COM Port) is installed – NNL's own software installs it; \
    without that software, download it from ftdichip.com → Drivers → VCP Drivers (Windows and macOS; Linux has it \
    built in). Alternatively use USB keyboard mode, which needs no driver.";

pub fn friendly_serial_error(err: &str, port: &str) -> String {
    let e = err.to_lowercase();
    let place = if port.is_empty() { "The port".to_string() } else { format!("Port {port}") };
    if e.contains("permission") || e.contains("access is denied") || e.contains("zugriff verweigert") {
        if cfg!(windows) {
            return format!(
                "{place} is in use by another program (e.g. NNL software or a second ParadigmShift window). \
                 Close it and try again."
            );
        }
        return format!(
            "No permission to open {}. Add your user to the 'dialout' group (sudo usermod -aG dialout $USER), \
             then log out and in again.",
            if port.is_empty() { "the port" } else { port }
        );
    }
    if e.contains("busy") || e.contains("in use") {
        return format!("{place} is in use by another program (e.g. NNL software). Close it and try again.");
    }
    if e.contains("filenotfound")
        || e.contains("no such file")
        || e.contains("cannot find")
        || e.contains("could not open port")
        || e.contains("not found")
        || e.contains("no such device")
    {
        return format!(
            "{place} was not found. Is the SyncBox connected by USB and switched on? \
             Click “Refresh” or “Auto-detect”. If the SyncBox never shows up as a port, FTDI's VCP driver is \
             missing (see the note in Hardware → SyncBox) – or use USB keyboard mode."
        );
    }
    err.to_string()
}

fn open_port(port: &str, baud: u32, timeout_ms: u64) -> Result<Box<dyn serialport::SerialPort>, String> {
    serialport::new(port, baud)
        .timeout(Duration::from_millis(timeout_ms))
        .open()
        .map_err(|e| friendly_serial_error(&e.to_string(), port))
}

// =============================================================================
// Stream analysis (auto-detect fallback)
// =============================================================================
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamAnalysis {
    pub chars: BTreeMap<char, usize>,
    pub trigger: Option<char>,
    pub tr: Option<f64>,
    pub buttons: Vec<char>,
    pub printable: f64,
}

/// Which character is the scanner trigger, and what is the TR? The trigger arrives at a steady
/// rhythm; characters that come irregularly are grip-button presses.
pub fn analyse_stream(events: &[(char, f64)], trigger_hint: &str) -> StreamAnalysis {
    let raw: Vec<char> = events.iter().map(|e| e.0).collect();
    let ok = raw.iter().filter(|c| (' '..='~').contains(*c) || **c == '\r' || **c == '\n').count();
    let printable = if raw.is_empty() { 0.0 } else { ok as f64 / raw.len() as f64 };
    let chars: Vec<(char, f64)> = events.iter().filter(|(c, _)| *c > ' ' && *c <= '~').cloned().collect();
    let mut counts: BTreeMap<char, usize> = BTreeMap::new();
    let mut order: Vec<char> = vec![];
    for (c, _) in &chars {
        if !counts.contains_key(c) {
            order.push(*c);
        }
        *counts.entry(*c).or_default() += 1;
    }
    let mut regular: BTreeMap<char, f64> = BTreeMap::new();
    for (&ch, &n) in &counts {
        if n < 3 {
            continue;
        }
        let ts: Vec<f64> = chars.iter().filter(|(c, _)| *c == ch).map(|(_, t)| *t).collect();
        let d: Vec<f64> = ts.windows(2).map(|w| w[1] - w[0]).collect();
        let mean = d.iter().sum::<f64>() / d.len() as f64;
        let sd = (d.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / d.len() as f64).sqrt();
        if mean > 0.2 && sd / mean < 0.1 {
            regular.insert(ch, mean);
        }
    }
    let hint = trigger_hint.chars().next();
    let trigger = match hint {
        Some(h) if regular.contains_key(&h) || (counts.contains_key(&h) && regular.is_empty()) => Some(h),
        _ => regular.keys().max_by_key(|c| counts[*c]).copied(),
    };
    StreamAnalysis {
        tr: trigger.and_then(|t| regular.get(&t).copied()),
        buttons: order.into_iter().filter(|c| Some(*c) != trigger).collect(),
        chars: counts,
        trigger,
        printable,
    }
}

// =============================================================================
// NordicNeuroLab SyncBox serial protocol (from NNL's py_syncbox)
// =============================================================================
// Every command byte is echoed by the SyncBox. "C" = computer mode, "R" + 48-byte configuration,
// "S" = start, "A" = stop, "D" = back to manual mode. During a session it sends "s" for each
// trigger and "a"–"d" for the ResponseGrip buttons.
pub const NNL_COMPUTER_MODE: &[u8] = b"C";
pub const NNL_CONFIGURE: &[u8] = b"R";
pub const NNL_START: &[u8] = b"S";
pub const NNL_STOP: &[u8] = b"A";
pub const NNL_MANUAL: &[u8] = b"D";
pub const NNL_TRIGGER: &str = "s";

pub fn nnl_button_key(name: &str) -> Option<&'static str> {
    match name {
        "Left thumb" => Some("a"),
        "Left index" => Some("b"),
        "Right index" => Some("c"),
        "Right thumb" => Some("d"),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NnlConfig {
    pub volumes: u32,
    pub tr_ms: u32,
    pub simulation: bool,
    pub slices: u32,
    pub pulse_ms: u32,
}

/// The 48-byte configuration: 12 values as 4-digit ASCII, in NNL's order.
pub fn nnl_payload(c: &NnlConfig) -> Vec<u8> {
    let vals: [i64; 12] = [
        0,
        c.volumes as i64,
        c.slices as i64,
        c.pulse_ms as i64,
        c.tr_ms as i64,
        1, // trigger slice
        1, // trigger volume
        0,
        0,
        0, // every slice
        0, // every volume
        if c.simulation { 0 } else { 1 },
    ];
    vals.iter().map(|v| format!("{:04}", (*v).clamp(0, 9999))).collect::<String>().into_bytes()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Send `payload` and wait until the SyncBox echoes it.
pub fn nnl_command(port: &mut dyn serialport::SerialPort, payload: &[u8], timeout: f64) -> bool {
    if port.write_all(payload).is_err() {
        return false;
    }
    let _ = port.flush();
    let mut buf: Vec<u8> = vec![];
    let end = now() + timeout;
    let mut tmp = [0u8; 64];
    while now() < end {
        let want = payload.len().saturating_sub(buf.len()).clamp(1, 64);
        match port.read(&mut tmp[..want]) {
            Ok(n) if n > 0 => {
                buf.extend_from_slice(&tmp[..n]);
                if contains(&buf, payload) {
                    return true;
                }
            }
            _ => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    false
}

/// Is there a SyncBox on `port`? (It answers the computer-mode command; it is put back to manual mode.)
pub fn nnl_probe(port: &str, baud: u32, timeout: f64) -> bool {
    let Ok(mut ser) = serialport::new(port, baud).timeout(Duration::from_millis(20)).open() else {
        return false;
    };
    let _ = ser.clear(serialport::ClearBuffer::Input);
    if nnl_command(ser.as_mut(), NNL_COMPUTER_MODE, timeout) {
        nnl_command(ser.as_mut(), NNL_MANUAL, timeout);
        return true;
    }
    false
}

// =============================================================================
// Serial reader thread
// =============================================================================
#[derive(Clone, Debug)]
pub enum HwEvent {
    Received(char, f64),
    SimTrigger(f64),
    Failed(String),
    Status(String),
}

pub struct SerialReader {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

fn handshake(ser: &mut dyn serialport::SerialPort, port: &str, c: &NnlConfig, tx: &Sender<HwEvent>) -> bool {
    let cfg = nnl_payload(c);
    let steps: [(&[u8], &str); 4] = [
        (NNL_COMPUTER_MODE, "computer mode"),
        (NNL_CONFIGURE, "configure"),
        (&cfg, "configuration"),
        (NNL_START, "start"),
    ];
    let _ = ser.clear(serialport::ClearBuffer::Input);
    for (payload, name) in steps {
        if !nnl_command(ser, payload, 1.0) {
            let _ = tx.send(HwEvent::Failed(format!(
                "The SyncBox on {port} did not confirm “{name}”. Is it switched on and set to serial mode, and is \
                 no other program (e.g. NNL software) using it?"
            )));
            nnl_command(ser, NNL_MANUAL, 0.5);
            return false;
        }
    }
    let mode = if c.simulation { "simulation" } else { "synchronisation" };
    let tr = if c.simulation { format!(" at TR {} s", crate::model::fmt_g(c.tr_ms as f64 / 1000.0)) } else { String::new() };
    let _ = tx.send(HwEvent::Status(format!("SyncBox on {port}: computer mode, {mode}{tr} – ready.")));
    true
}

impl SerialReader {
    /// `control`: run NNL's protocol (computer mode → configure → start) and stop / return to
    /// manual mode when finished.
    pub fn start(port: String, baud: u32, control: Option<NnlConfig>, tx: Sender<HwEvent>, waker: Waker) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        let handle = std::thread::Builder::new()
            .name("serial-reader".into())
            .spawn(move || {
                let send = |e: HwEvent| {
                    let _ = tx.send(e);
                    waker();
                };
                let mut ser = match open_port(&port, baud, 20) {
                    Ok(s) => s,
                    Err(e) => {
                        send(HwEvent::Failed(e));
                        return;
                    }
                };
                if let Some(c) = &control {
                    if !handshake(ser.as_mut(), &port, c, &tx) {
                        waker();
                        return;
                    }
                    waker();
                } else {
                    let _ = ser.clear(serialport::ClearBuffer::Input);
                }
                let _ = ser.set_timeout(Duration::from_millis(1));
                let mut buf = [0u8; 64];
                while !st.load(Ordering::Relaxed) {
                    match ser.read(&mut buf) {
                        Ok(n) if n > 0 => {
                            let t = now();
                            for &byte in &buf[..n] {
                                let _ = tx.send(HwEvent::Received(byte as char, t));
                            }
                            waker();
                        }
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(e) => {
                            send(HwEvent::Failed(friendly_serial_error(&e.to_string(), &port)));
                            return;
                        }
                    }
                }
                if control.is_some() {
                    // end the session and give the box back to its own menu
                    let _ = ser.set_timeout(Duration::from_millis(20));
                    nnl_command(ser.as_mut(), NNL_STOP, 0.5);
                    nnl_command(ser.as_mut(), NNL_MANUAL, 0.5);
                }
            })
            .ok();
        SerialReader { stop, handle }
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for SerialReader {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Generates scanner triggers at the TR (software simulation), time-stamped at the scheduled time.
struct Simulator {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Simulator {
    fn start(tr: f64, tx: Sender<HwEvent>, waker: Waker) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        let handle = std::thread::Builder::new()
            .name("scanner-sim".into())
            .spawn(move || {
                let t0 = now();
                let mut k = 0u64;
                while !st.load(Ordering::Relaxed) {
                    let due = t0 + k as f64 * tr;
                    let wait = due - now();
                    if wait > 0.0 {
                        std::thread::sleep(Duration::from_secs_f64(wait.min(0.01)));
                        continue;
                    }
                    let _ = tx.send(HwEvent::SimTrigger(due));
                    waker();
                    k += 1;
                }
            })
            .ok();
        Simulator { stop, handle }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        self.stop();
    }
}

// =============================================================================
// Input router
// =============================================================================
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    Trigger(f64),
    /// a grip button (by name)
    Button(String, f64),
    /// every non-trigger key/character: key, button name ("" = unassigned), time
    Pressed(String, String, f64),
    /// a key that is not assigned to any button
    Other(String, f64),
    Error(String),
    Status(String),
}

pub struct InputRouter {
    pub hw: Hardware,
    pub simulate_scanner: bool,
    /// a SyncBox we control simulates the scanner itself – then its triggers are real input
    pub hardware_sim: bool,
    pub tr: f64,
    pub volumes: u32,
    reader: Option<SerialReader>,
    sim: Option<Simulator>,
    tx: Sender<HwEvent>,
    rx: Receiver<HwEvent>,
    waker: Waker,
    out: Vec<Input>,
}

impl InputRouter {
    pub fn new(hw: Hardware, simulate_scanner: bool, tr: f64, volumes: u32, waker: Waker) -> Self {
        let (tx, rx) = channel();
        let hardware_sim = simulate_scanner && hw.sync_mode == "serial" && hw.syncbox_control;
        InputRouter {
            hw,
            simulate_scanner,
            hardware_sim,
            tr,
            volumes,
            reader: None,
            sim: None,
            tx,
            rx,
            waker,
            out: vec![],
        }
    }

    pub fn start(&mut self) {
        if self.hw.sync_mode != "serial" {
            return;
        }
        if self.hw.serial_port.is_empty() {
            self.out.push(Input::Error(
                "Serial mode is selected but no port is set – open Hardware and click Auto-detect.".into(),
            ));
            return;
        }
        let control = if self.hw.syncbox_control {
            Some(NnlConfig {
                volumes: self.volumes,
                tr_ms: (self.tr * 1000.0).round() as u32,
                simulation: self.hardware_sim,
                slices: self.hw.syncbox_slices,
                pulse_ms: self.hw.syncbox_pulse_ms,
            })
        } else {
            None
        };
        self.reader = Some(SerialReader::start(
            self.hw.serial_port.clone(),
            self.hw.baud_rate,
            control,
            self.tx.clone(),
            self.waker.clone(),
        ));
    }

    /// Software scanner simulation (not used when a controlled SyncBox simulates).
    pub fn start_simulation(&mut self) {
        if self.simulate_scanner && self.tr > 0.0 && !self.hardware_sim && self.sim.is_none() {
            self.sim = Some(Simulator::start(self.tr, self.tx.clone(), self.waker.clone()));
        }
    }

    pub fn stop_simulation(&mut self) {
        if let Some(mut s) = self.sim.take() {
            s.stop();
        }
    }

    pub fn stop(&mut self) {
        self.stop_simulation();
        if let Some(mut r) = self.reader.take() {
            r.stop();
        }
    }

    pub fn is_listening(&self) -> bool {
        self.reader.is_some()
    }

    /// Whether keys typed on the computer's keyboard count. In serial mode they don't: triggers and
    /// grip presses then come from the SyncBox only, so a key pressed by accident is never taken
    /// for a scanner pulse or an answer.
    pub fn accepts_keyboard(&self) -> bool {
        self.hw.sync_mode != "serial"
    }

    /// Route a key/character coming from the keyboard (check `accepts_keyboard` first) or the
    /// serial port.
    pub fn feed(&mut self, text: &str, t: f64) {
        if text.is_empty() {
            return;
        }
        if text.to_lowercase() == self.hw.trigger_key.to_lowercase() {
            if !self.simulate_scanner || self.hardware_sim {
                self.out.push(Input::Trigger(t));
            }
            return;
        }
        let name = self.hw.button_for_key(text);
        self.out.push(Input::Pressed(text.to_string(), name.clone().unwrap_or_default(), t));
        match name {
            Some(n) => self.out.push(Input::Button(n, t)),
            None => self.out.push(Input::Other(text.to_string(), t)),
        }
    }

    /// Everything that arrived since the last call, in order.
    pub fn poll(&mut self) -> Vec<Input> {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                HwEvent::Received(c, t) => self.feed(&c.to_string(), t),
                HwEvent::SimTrigger(t) => self.out.push(Input::Trigger(t)),
                HwEvent::Failed(m) => {
                    self.reader = None;
                    self.out.push(Input::Error(m))
                }
                HwEvent::Status(m) => self.out.push(Input::Status(m)),
            }
        }
        std::mem::take(&mut self.out)
    }
}

impl Drop for InputRouter {
    fn drop(&mut self) {
        self.stop();
    }
}

// =============================================================================
// Auto-detection
// =============================================================================
#[derive(Clone, Debug, PartialEq)]
pub struct ScanResult {
    pub port: String,
    pub baud: u32,
    pub description: String,
    pub nnl: bool,
    pub analysis: StreamAnalysis,
}

pub enum ScanMsg {
    Progress(String),
    Done(Vec<ScanResult>, Vec<String>),
}

/// Auto-detect: ask every port whether it is an NNL SyncBox; if none answers, listen on every port
/// at the common baud rates and find the one that talks (scanner running / grips pressed).
pub struct PortScanner {
    stop: Arc<AtomicBool>,
    pub rx: Receiver<ScanMsg>,
    handle: Option<JoinHandle<()>>,
}

impl PortScanner {
    pub fn start(ports: Option<Vec<String>>, listen_s: f64, trigger_hint: String, bauds: Vec<u32>, waker: Waker) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        let (tx, rx) = channel();
        let handle = std::thread::Builder::new()
            .name("port-scanner".into())
            .spawn(move || {
                let send = |m: ScanMsg| {
                    let _ = tx.send(m);
                    waker();
                };
                let info: Vec<PortInfo> = describe_ports();
                let desc = |dev: &str| info.iter().find(|p| p.device == dev).map(|p| p.description.clone()).unwrap_or_default();
                let ports = ports.unwrap_or_else(|| info.iter().map(|p| p.device.clone()).collect());
                let mut errors: Vec<String> = vec![];
                if ports.is_empty() {
                    send(ScanMsg::Done(
                        vec![],
                        vec![format!(
                            "No serial ports found. Is the SyncBox connected by USB and switched on? If it is set to \
                             USB keyboard mode, it does not appear as a serial port.\n\n{FTDI_DRIVER_NOTE}"
                        )],
                    ));
                    return;
                }
                // 1) NNL handshake: a SyncBox answers the computer-mode command – no scanner needs to run
                for dev in &ports {
                    for &baud in &bauds {
                        if st.load(Ordering::Relaxed) {
                            break;
                        }
                        send(ScanMsg::Progress(format!("Asking {dev} at {baud} baud whether it is an NNL SyncBox …")));
                        if nnl_probe(dev, baud, 0.6) {
                            let res = ScanResult {
                                port: dev.clone(),
                                baud,
                                description: desc(dev),
                                nnl: true,
                                analysis: StreamAnalysis {
                                    trigger: NNL_TRIGGER.chars().next(),
                                    printable: 1.0,
                                    ..Default::default()
                                },
                            };
                            send(ScanMsg::Done(vec![res], errors));
                            return;
                        }
                    }
                }
                // 2) fallback: listen for a device that sends on its own
                let mut results = vec![];
                let mut dead: Vec<String> = vec![];
                for &baud in &bauds {
                    if st.load(Ordering::Relaxed) {
                        break;
                    }
                    let mut opened: Vec<(String, Box<dyn serialport::SerialPort>, Vec<(char, f64)>)> = vec![];
                    for dev in &ports {
                        if dead.contains(dev) {
                            continue;
                        }
                        match open_port(dev, baud, 0) {
                            Ok(p) => {
                                let _ = p.clear(serialport::ClearBuffer::Input);
                                opened.push((dev.clone(), p, vec![]));
                            }
                            Err(e) => {
                                dead.push(dev.clone());
                                errors.push(format!("{dev}: {e}"));
                            }
                        }
                    }
                    if opened.is_empty() {
                        break;
                    }
                    let t_end = now() + listen_s;
                    let names: Vec<String> = opened.iter().map(|o| o.0.clone()).collect();
                    let mut last_msg = 0.0;
                    while now() < t_end && !st.load(Ordering::Relaxed) {
                        if now() - last_msg > 0.25 {
                            send(ScanMsg::Progress(format!(
                                "Listening on {} at {baud} baud …  {:.0} s",
                                names.join(", "),
                                t_end - now()
                            )));
                            last_msg = now();
                        }
                        for (dev, ser, ev) in opened.iter_mut() {
                            let n = ser.bytes_to_read().unwrap_or(0) as usize;
                            if n == 0 {
                                continue;
                            }
                            let mut buf = vec![0u8; n.min(256)];
                            match ser.read(&mut buf) {
                                Ok(k) => {
                                    let t = now();
                                    ev.extend(buf[..k].iter().map(|b| (*b as char, t)));
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                                Err(e) => errors.push(format!("{dev}: {}", friendly_serial_error(&e.to_string(), dev))),
                            }
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    for (dev, _ser, ev) in opened {
                        if ev.is_empty() {
                            continue;
                        }
                        let a = analyse_stream(&ev, &trigger_hint);
                        if a.printable >= 0.9 {
                            results.push(ScanResult { port: dev.clone(), baud, description: desc(&dev), nnl: false, analysis: a });
                        } else {
                            send(ScanMsg::Progress(format!("{dev}: data at {baud} baud looks garbled – trying another speed")));
                        }
                    }
                    if !results.is_empty() {
                        break;
                    }
                }
                send(ScanMsg::Done(results, errors));
            })
            .ok();
        PortScanner { stop, rx, handle }
    }

    pub fn cancel(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for PortScanner {
    fn drop(&mut self) {
        self.cancel();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyse_stream_finds_trigger_and_tr() {
        let mut ev: Vec<(char, f64)> = (0..6).map(|i| ('s', 2.0 * i as f64)).collect();
        ev.extend([('1', 3.1), ('2', 5.7), ('1', 8.2)]);
        ev.sort_by(|a, b| a.1.total_cmp(&b.1));
        let a = analyse_stream(&ev, "");
        assert_eq!(a.trigger, Some('s'));
        assert!((a.tr.unwrap() - 2.0).abs() < 1e-9);
        assert_eq!(a.buttons, vec!['1', '2']);
        assert_eq!(a.printable, 1.0);
        let garbled: Vec<(char, f64)> =
            [0x80u8, 0xF3, 0x00, 0x9C, 0xFF].iter().enumerate().map(|(i, c)| (*c as char, i as f64 * 0.1)).collect();
        assert!(analyse_stream(&garbled, "").printable < 0.5);
        assert_eq!(analyse_stream(&[('5', 0.0)], "5").trigger, Some('5'));
    }

    #[test]
    fn friendly_errors() {
        assert!(friendly_serial_error("could not open port 'COM9': FileNotFoundError(2, ...)", "COM9").contains("not found"));
        let m = friendly_serial_error("Permission denied", "/dev/ttyUSB0");
        assert!(m.contains("dialout") || m.contains("in use"));
        assert!(friendly_serial_error("Device or resource busy", "COM3").contains("in use"));
    }

    #[test]
    fn nnl_payload_format() {
        let p = nnl_payload(&NnlConfig { volumes: 150, tr_ms: 2000, simulation: true, slices: 1, pulse_ms: 100 });
        assert_eq!(p.len(), 48);
        assert_eq!(&p[..8], b"00000150");
        assert_eq!(&p[16..20], b"2000");
        assert_eq!(&p[44..48], b"0000");
    }

    #[test]
    fn router_maps_keys() {
        let mut r = InputRouter::new(Hardware::default(), false, 2.0, 10, no_waker());
        r.feed("s", 1.0);
        r.feed("b", 2.0);
        r.feed("x", 3.0);
        let ev = r.poll();
        assert_eq!(ev[0], Input::Trigger(1.0));
        assert_eq!(ev[1], Input::Pressed("b".into(), "Left index".into(), 2.0));
        assert_eq!(ev[2], Input::Button("Left index".into(), 2.0));
        assert_eq!(ev[4], Input::Other("x".into(), 3.0));
        // in software simulation the trigger key is ignored (triggers come from the simulator)
        let mut sim = InputRouter::new(Hardware::default(), true, 0.05, 10, no_waker());
        sim.feed("s", 1.0);
        assert!(sim.poll().is_empty());
        sim.start_simulation();
        std::thread::sleep(Duration::from_millis(180));
        let n = sim.poll().iter().filter(|e| matches!(e, Input::Trigger(_))).count();
        assert!((3..=5).contains(&n), "{n} simulated triggers");
    }

    /// A pseudo-terminal that speaks NNL's SyncBox protocol (Linux/macOS).
    #[cfg(unix)]
    #[test]
    fn nnl_handshake_with_fake_syncbox() {
        use serialport::{SerialPort, TTYPort};
        let (mut master, slave) = TTYPort::pair().expect("pty");
        let dev = slave.name().unwrap();
        // fake SyncBox: echo every command; once configured with simulation and started, send triggers
        let fake = std::thread::spawn(move || {
            let mut seen: Vec<String> = vec![];
            let mut buf = [0u8; 64];
            let mut pending: Vec<u8> = vec![];
            let mut config: Option<Vec<u8>> = None;
            let end = now() + 5.0;
            master.set_timeout(Duration::from_millis(5)).ok();
            while now() < end {
                if let Ok(n) = master.read(&mut buf) {
                    pending.extend_from_slice(&buf[..n]);
                }
                while !pending.is_empty() {
                    if seen.last().map(|s| s == "R").unwrap_or(false) && config.is_none() {
                        if pending.len() < 48 {
                            break;
                        }
                        let cfg: Vec<u8> = pending.drain(..48).collect();
                        master.write_all(&cfg).ok();
                        config = Some(cfg);
                        seen.push("config".into());
                        continue;
                    }
                    let c = pending.remove(0);
                    master.write_all(&[c]).ok();
                    seen.push((c as char).to_string());
                    if c == b'S' {
                        for _ in 0..3 {
                            master.write_all(b"s").ok();
                            std::thread::sleep(Duration::from_millis(30));
                        }
                        master.write_all(b"c").ok();
                    }
                    if c == b'D' {
                        return seen;
                    }
                }
            }
            seen
        });
        let (tx, rx) = channel();
        let mut reader = SerialReader::start(
            dev,
            57600,
            Some(NnlConfig { volumes: 20, tr_ms: 2000, simulation: false, slices: 1, pulse_ms: 100 }),
            tx,
            no_waker(),
        );
        let mut got = vec![];
        let end = now() + 3.0;
        while now() < end && got.iter().filter(|e| matches!(e, HwEvent::Received(_, _))).count() < 4 {
            if let Ok(e) = rx.recv_timeout(Duration::from_millis(50)) {
                got.push(e);
            }
        }
        reader.stop();
        let seen = fake.join().unwrap();
        assert!(got.iter().any(|e| matches!(e, HwEvent::Status(_))), "{got:?}");
        let chars: String = got.iter().filter_map(|e| if let HwEvent::Received(c, _) = e { Some(*c) } else { None }).collect();
        assert_eq!(chars, "sssc");
        assert_eq!(seen, vec!["C", "R", "config", "S", "A", "D"]);
    }
}
