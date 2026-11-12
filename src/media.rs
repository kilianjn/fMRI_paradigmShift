//! Sound and video.
//!
//! * Audio output devices (listed by name, chosen in Hardware → Audio – off by default).
//! * Audio files – and the sound track of videos – are decoded completely into memory before a
//!   run (no disk access while running) and played on a dedicated audio thread, started by the
//!   run clock at the block's onset and cut at its end.
//! * Videos (MP4 / MOV with H.264 – baseline, main and high profile) are decoded on background
//!   threads a few frames ahead; the participant screen shows the frame that belongs to the time
//!   since the block's onset.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use egui::{ColorImage, TextureHandle};

use crate::hardware::now;

pub const AUDIO_EXTS: &[&str] = &["wav", "mp3", "ogg", "flac", "m4a", "aac"];
pub const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "mov"];

fn ext(path: &str) -> String {
    Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase()
}

pub fn is_audio_file(path: &str) -> bool {
    AUDIO_EXTS.contains(&ext(path).as_str())
}

pub fn is_video_file(path: &str) -> bool {
    VIDEO_EXTS.contains(&ext(path).as_str())
}

// =============================================================================
// Audio files
// =============================================================================
/// A decoded sound: interleaved f32 samples.
#[derive(Clone)]
pub struct AudioClip {
    pub channels: u16,
    pub rate: u32,
    pub samples: Arc<[f32]>,
}

impl AudioClip {
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / (self.channels.max(1) as f64 * self.rate.max(1) as f64)
    }

    /// A short sine tone (for "Test sound").
    pub fn tone(freq: f32, secs: f32) -> AudioClip {
        let rate = 48_000u32;
        let n = (rate as f32 * secs) as usize;
        let samples: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                // 20 ms fade in / out, no clicks
                let env = (t / 0.02).min(1.0).min((secs - t) / 0.02).max(0.0);
                (t * freq * std::f32::consts::TAU).sin() * 0.3 * env
            })
            .collect();
        AudioClip { channels: 1, rate, samples: samples.into() }
    }
}

fn open_format(path: &str) -> Result<Box<dyn symphonia::core::formats::FormatReader>, String> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;
    let file = std::fs::File::open(path).map_err(|e| format!("Could not open {}: {e}", basename(path)))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    let e = ext(path);
    if !e.is_empty() {
        hint.with_extension(&e);
    }
    symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map(|p| p.format)
        .map_err(|e| format!("{}: not a sound file this app can read ({e})", basename(path)))
}

fn basename(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// The first sound track of a file (an audio file, or the sound of a video).
fn sound_track(format: &dyn symphonia::core::formats::FormatReader) -> Option<symphonia::core::formats::Track> {
    use symphonia::core::codecs::CODEC_TYPE_NULL;
    format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL && t.codec_params.sample_rate.is_some()).cloned()
}

/// Length of a sound file in seconds, without decoding it (None if unknown).
pub fn audio_duration(path: &str) -> Option<f64> {
    let format = open_format(path).ok()?;
    let t = sound_track(format.as_ref())?;
    let n = t.codec_params.n_frames?;
    let rate = t.codec_params.sample_rate?;
    Some(n as f64 / rate.max(1) as f64)
}

/// Decode the whole sound of a file into memory. Only the sound track's packets are decoded (a
/// video's picture packets are skipped).
pub fn decode_audio(path: &str) -> Result<AudioClip, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::errors::Error as SymErr;
    let mut format = open_format(path)?;
    let track = sound_track(format.as_ref()).ok_or_else(|| format!("{}: has no sound", basename(path)))?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("{}: sound format not supported ({e})", basename(path)))?;
    let (mut channels, mut rate) = (0u16, 0u32);
    let mut samples: Vec<f32> = vec![];
    let mut sb: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymErr::IoError(_)) | Err(SymErr::ResetRequired) => break,
            Err(e) => {
                if samples.is_empty() {
                    return Err(format!("{}: {e}", basename(path)));
                }
                break;
            }
        };
        if packet.track_id() != track.id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let spec = *buf.spec();
                channels = spec.channels.count() as u16;
                rate = spec.rate;
                let needed = buf.capacity() as u64;
                if sb.as_ref().map(|b| b.capacity() < needed as usize).unwrap_or(true) {
                    sb = Some(SampleBuffer::<f32>::new(needed, spec));
                }
                if let Some(b) = sb.as_mut() {
                    b.copy_interleaved_ref(buf);
                    samples.extend_from_slice(b.samples());
                }
            }
            Err(SymErr::DecodeError(_)) => continue,
            Err(e) => return Err(format!("{}: {e}", basename(path))),
        }
    }
    if samples.is_empty() || channels == 0 || rate == 0 {
        return Err(format!("{}: no sound could be decoded", basename(path)));
    }
    Ok(AudioClip { channels, rate, samples: samples.into() })
}

/// Plays a clip from memory (no copy), optionally cut after `limit` samples.
struct ClipSource {
    clip: AudioClip,
    pos: usize,
    end: usize,
}

impl Iterator for ClipSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.pos >= self.end {
            return None;
        }
        let s = self.clip.samples[self.pos];
        self.pos += 1;
        Some(s)
    }
}

impl rodio::Source for ClipSource {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.end - self.pos)
    }
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(self.clip.channels.max(1)).unwrap_or(std::num::NonZero::<u16>::MIN)
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(self.clip.rate.max(1)).unwrap_or(std::num::NonZero::<u32>::MIN)
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.clip.duration()))
    }
}

// =============================================================================
// Audio output
// =============================================================================
/// Value of `Hardware::audio_device` for the system's default output.
pub const DEFAULT_OUTPUT: &str = "default";

#[derive(Clone, Debug, PartialEq)]
pub struct OutputDevice {
    /// cpal device id (stable across runs), or `DEFAULT_OUTPUT`
    pub id: String,
    pub name: String,
}

/// The sound outputs of this computer (speakers, headphones, HDMI, USB sound cards …).
pub fn output_devices() -> Vec<OutputDevice> {
    use rodio::cpal::traits::HostTrait;
    use rodio::DeviceTrait;
    let host = rodio::cpal::default_host();
    let mut out = vec![];
    if let Ok(devs) = host.output_devices() {
        for d in devs {
            let (Ok(id), Ok(desc)) = (d.id(), d.description()) else { continue };
            let id = id.to_string();
            if id.ends_with(":null") {
                continue; // "discard all samples" pseudo device
            }
            out.push(OutputDevice { id, name: desc.name().to_string() });
        }
    }
    out
}

fn open_device(id: &str) -> Result<rodio::MixerDeviceSink, String> {
    use rodio::cpal::traits::HostTrait;
    let host = rodio::cpal::default_host();
    let device = if id == DEFAULT_OUTPUT {
        host.default_output_device().ok_or("This computer has no sound output.")?
    } else {
        let parsed: Option<rodio::cpal::DeviceId> = id.parse().ok();
        parsed.and_then(|d| host.device_by_id(&d)).ok_or("The chosen sound output is not connected – choose it again in Hardware → Audio.")?
    };
    rodio::DeviceSinkBuilder::from_device(device)
        .and_then(|b| b.open_sink_or_fallback())
        .map_err(|e| format!("Could not open the sound output: {e}"))
}

enum AudioCmd {
    Play { key: String, clip: AudioClip, volume: f32, limit: Option<f64> },
    StopAll,
    Quit,
}

/// The sound output while a run (or a sound test) is going: owns the device on its own thread.
pub struct AudioEngine {
    tx: Sender<AudioCmd>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl AudioEngine {
    /// Open the output `device` (a device id, or `DEFAULT_OUTPUT`).
    pub fn start(device: &str) -> Result<AudioEngine, String> {
        let (tx, rx) = channel::<AudioCmd>();
        let (ok_tx, ok_rx) = channel::<Result<(), String>>();
        let id = device.to_string();
        let handle = std::thread::Builder::new()
            .name("audio-output".into())
            .spawn(move || {
                let sink = match open_device(&id) {
                    Ok(s) => {
                        let _ = ok_tx.send(Ok(()));
                        s
                    }
                    Err(e) => {
                        let _ = ok_tx.send(Err(e));
                        return;
                    }
                };
                audio_loop(sink, rx);
            })
            .map_err(|e| e.to_string())?;
        match ok_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(AudioEngine { tx, handle: Some(handle) }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("The sound output did not respond.".into()),
        }
    }

    /// Start `clip` now (replacing what played under the same key), cut after `limit` seconds.
    pub fn play(&self, key: &str, clip: AudioClip, volume: f32, limit: Option<f64>) {
        let _ = self.tx.send(AudioCmd::Play { key: key.to_string(), clip, volume, limit });
    }

    pub fn stop_all(&self) {
        let _ = self.tx.send(AudioCmd::StopAll);
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.tx.send(AudioCmd::Quit);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn audio_loop(sink: rodio::MixerDeviceSink, rx: Receiver<AudioCmd>) {
    let mut players: HashMap<String, rodio::Player> = HashMap::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(AudioCmd::Play { key, clip, volume, limit }) => {
                let mut end = clip.samples.len();
                if let Some(l) = limit {
                    // whole frames only
                    let n = ((l.max(0.0) * clip.rate as f64) as usize) * clip.channels.max(1) as usize;
                    end = end.min(n);
                }
                let player = rodio::Player::connect_new(sink.mixer());
                player.set_volume(volume.clamp(0.0, 2.0));
                player.append(ClipSource { clip, pos: 0, end });
                // replacing a player drops (and stops) the old one
                players.insert(key, player);
            }
            Ok(AudioCmd::StopAll) => {
                for (_, p) in players.drain() {
                    p.stop();
                }
            }
            Ok(AudioCmd::Quit) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        players.retain(|_, p| !p.empty());
    }
    for (_, p) in players.drain() {
        p.stop();
    }
    drop(sink);
}

/// Play a short tone on `device` (Hardware → Audio → Test sound). Returns the engine, which must be
/// kept alive while the tone plays.
pub fn test_sound(device: &str) -> Result<AudioEngine, String> {
    let e = AudioEngine::start(device)?;
    e.play("test", AudioClip::tone(660.0, 0.7), 1.0, None);
    Ok(e)
}

// =============================================================================
// Video files (H.264 in MP4 / MOV)
// =============================================================================
#[derive(Clone, Debug, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    pub fps: f64,
}

/// One H.264 track of an MP4/MOV file, ready to be decoded.
struct H264Track {
    // read straight from the file: only the header and the samples that are decoded are loaded
    mp4: mp4::Mp4Reader<std::io::BufReader<std::fs::File>>,
    track_id: u32,
    length_size: usize,
    sps: Vec<Vec<u8>>,
    pps: Vec<Vec<u8>>,
    samples: u32,
    timescale: f64,
    info: VideoInfo,
}

fn open_h264(path: &str) -> Result<H264Track, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Could not open {}: {e}", basename(path)))?;
    let size = file.metadata().map(|m| m.len()).map_err(|e| format!("Could not open {}: {e}", basename(path)))?;
    let mp4 = mp4::Mp4Reader::read_header(std::io::BufReader::new(file), size)
        .map_err(|e| format!("{}: not an MP4/MOV video this app can read ({e})", basename(path)))?;
    let (track_id, track) = mp4
        .tracks()
        .iter()
        .find(|(_, t)| t.media_type().ok() == Some(mp4::MediaType::H264))
        .map(|(i, t)| (*i, t))
        .ok_or_else(|| {
            format!(
                "{}: the video is not H.264. Convert it to an H.264 MP4 (e.g. with HandBrake or “ffmpeg -i in -c:v libx264 out.mp4”).",
                basename(path)
            )
        })?;
    let avcc = &track.trak.mdia.minf.stbl.stsd.avc1.as_ref().ok_or_else(|| format!("{}: H.264 settings missing", basename(path)))?.avcc;
    let length_size = (avcc.length_size_minus_one + 1) as usize;
    let sps = avcc.sequence_parameter_sets.iter().map(|v| v.bytes.clone()).collect();
    let pps = avcc.picture_parameter_sets.iter().map(|v| v.bytes.clone()).collect();
    let timescale = track.timescale().max(1) as f64;
    let info = VideoInfo {
        width: track.width() as u32,
        height: track.height() as u32,
        duration: track.duration().as_secs_f64(),
        fps: track.frame_rate(),
    };
    let samples = track.sample_count();
    Ok(H264Track { mp4, track_id, length_size, sps, pps, samples, timescale, info })
}

/// Size, length and frame rate of a video (fails with a readable message for other formats).
pub fn probe_video(path: &str) -> Result<VideoInfo, String> {
    open_h264(path).map(|t| t.info)
}

/// MP4 stores NAL units with a length prefix; the decoder wants Annex B start codes. Key frames get
/// the stream's parameter sets in front.
fn to_annex_b(sample: &[u8], length_size: usize, sps: &[Vec<u8>], pps: &[Vec<u8>], out: &mut Vec<u8>) {
    out.clear();
    let mut s = sample;
    let mut params_done = false;
    while s.len() > length_size {
        let mut n = 0usize;
        for b in &s[..length_size] {
            n = (n << 8) | *b as usize;
        }
        s = &s[length_size..];
        if n == 0 || n > s.len() {
            break;
        }
        let nal = &s[..n];
        if nal[0] & 0x1F == 5 && !params_done {
            for p in sps.iter().chain(pps.iter()) {
                out.extend_from_slice(&[0, 0, 0, 1]);
                out.extend_from_slice(p);
            }
            params_done = true;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
        s = &s[n..];
    }
}

/// Frames decoded ahead of time (small: a few frames).
const AHEAD: usize = 6;

#[derive(Default)]
struct StreamState {
    frames: VecDeque<(f64, ColorImage)>,
    done: bool,
    error: Option<String>,
}

struct Stream {
    state: Arc<Mutex<StreamState>>,
    stop: Arc<AtomicBool>,
}

impl Stream {
    /// Decode `path` from the key frame before `from` (seconds) on; frames before `from` are skipped.
    fn start(path: &str, from: f64) -> Stream {
        let state = Arc::new(Mutex::new(StreamState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp, p) = (state.clone(), stop.clone(), path.to_string());
        let _ = std::thread::Builder::new().name("video-decoder".into()).spawn(move || {
            let r = decode_video(&p, from, &st, &sp);
            if let Ok(mut s) = st.lock() {
                s.done = true;
                if let Err(e) = r {
                    s.error = Some(e);
                }
            }
            if let Some(c) = crate::stimulus::ctx() {
                c.request_repaint();
            }
        });
        Stream { state, stop }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn decode_video(path: &str, from: f64, st: &Mutex<StreamState>, stop: &AtomicBool) -> Result<(), String> {
    use openh264::decoder::{Decoder, DecoderConfig, Flush};
    use openh264::formats::YUVSource;
    let mut t = open_h264(path)?;
    // composition (display) times of all samples; the first shown frame is t = 0
    let mut meta: Vec<(u32, f64, bool)> = Vec::with_capacity(t.samples as usize);
    for i in 1..=t.samples {
        if let Ok(Some(s)) = t.mp4.read_sample(t.track_id, i) {
            let ct = (s.start_time as f64 + s.rendering_offset as f64) / t.timescale;
            meta.push((i, ct, s.is_sync));
        }
    }
    let first = meta.iter().map(|m| m.1).fold(f64::INFINITY, f64::min);
    let first = if first.is_finite() { first } else { 0.0 };
    // start at the last key frame at or before `from` (seeking without decoding everything)
    let start = meta.iter().filter(|m| m.2 && m.1 - first <= from + 1e-6).map(|m| m.0).last().unwrap_or(1);
    let mut disp: Vec<f64> = meta.iter().filter(|m| m.0 >= start).map(|m| m.1 - first).collect();
    disp.sort_by(|a, b| a.total_cmp(b));
    let frame_dur = if t.info.fps > 0.0 { 1.0 / t.info.fps } else { 1.0 / 30.0 };
    let mut dec = Decoder::with_api_config(openh264::OpenH264API::from_source(), DecoderConfig::new().flush_after_decode(Flush::NoFlush))
        .map_err(|e| format!("Video decoder: {e}"))?;
    let mut buf = vec![];
    let mut rgba = vec![];
    let mut k = 0usize;
    // returns false when the stream was stopped
    let emit = |img: &openh264::decoder::DecodedYUV, k: usize, rgba: &mut Vec<u8>| -> bool {
        let pts = disp.get(k).copied().unwrap_or(k as f64 * frame_dur);
        if pts + frame_dur <= from {
            return !stop.load(Ordering::Relaxed); // before the wanted time: no conversion
        }
        let (w, h) = img.dimensions();
        rgba.resize(w * h * 4, 0);
        img.write_rgba8(rgba);
        let frame = ColorImage::from_rgba_unmultiplied([w, h], rgba);
        let mut frame = Some(frame);
        loop {
            if stop.load(Ordering::Relaxed) {
                return false;
            }
            if let Ok(mut s) = st.lock() {
                if s.frames.len() < AHEAD {
                    if let Some(f) = frame.take() {
                        s.frames.push_back((pts, f));
                    }
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(3));
        }
    };
    for i in start..=t.samples {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let Ok(Some(s)) = t.mp4.read_sample(t.track_id, i) else { continue };
        to_annex_b(&s.bytes, t.length_size, &t.sps, &t.pps, &mut buf);
        if let Ok(Some(img)) = dec.decode(&buf) {
            if !emit(&img, k, &mut rgba) {
                return Ok(());
            }
            k += 1;
        }
    }
    if let Ok(rest) = dec.flush_remaining() {
        for img in rest {
            if !emit(&img, k, &mut rgba) {
                return Ok(());
            }
            k += 1;
        }
    }
    if k == 0 {
        return Err(format!("{}: no picture could be decoded", basename(path)));
    }
    Ok(())
}

struct Playback {
    path: String,
    stream: Stream,
    from: f64,
    tex: Option<TextureHandle>,
    shown: Option<f64>,
    last_used: f64,
}

static VIDEOS: LazyLock<Mutex<HashMap<String, Playback>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub enum VideoStatus {
    Ready(crate::stimulus::Tex),
    Loading,
    Failed(String),
}

/// Start decoding a video ahead of its onset (e.g. 1.5 s before), so its first frame is ready.
pub fn video_prepare(key: &str, path: &str) {
    if path.is_empty() {
        return;
    }
    let Ok(mut v) = VIDEOS.lock() else { return };
    let t = now();
    match v.get_mut(key) {
        Some(p) if p.path == path => p.last_used = t,
        _ => {
            v.insert(key.to_string(), Playback { path: path.to_string(), stream: Stream::start(path, 0.0), from: 0.0, tex: None, shown: None, last_used: t });
        }
    }
}

/// Forget a playback (its decoder stops).
pub fn video_release(key: &str) {
    if let Ok(mut v) = VIDEOS.lock() {
        v.remove(key);
    }
}

/// Stop the decoders nobody asked for in the last two seconds (call once per frame).
pub fn video_gc() {
    let t = now();
    if let Ok(mut v) = VIDEOS.lock() {
        v.retain(|_, p| t - p.last_used < 2.0);
    }
}

/// The frame of video `path` at `t` seconds into it, for the playback `key` (a block id during a
/// run, "preview" in the editor). Jumps backwards or far ahead restart the decoder there.
pub fn video_frame(ctx: &egui::Context, key: &str, path: &str, t: f64) -> VideoStatus {
    if path.is_empty() {
        return VideoStatus::Failed("no video chosen".into());
    }
    let Ok(mut v) = VIDEOS.lock() else { return VideoStatus::Loading };
    let tnow = now();
    let restart = match v.get(key) {
        None => true,
        Some(p) if p.path != path => true,
        Some(p) => {
            let at = p.shown.unwrap_or(p.from);
            t + 0.05 < at || t > at + 2.0
        }
    };
    if restart {
        let from = t.max(0.0);
        v.insert(key.to_string(), Playback { path: path.to_string(), stream: Stream::start(path, from), from, tex: None, shown: None, last_used: tnow });
    }
    let Some(p) = v.get_mut(key) else { return VideoStatus::Loading };
    p.last_used = tnow;
    let (newest, error, done) = {
        let Ok(mut s) = p.stream.state.lock() else { return VideoStatus::Loading };
        let mut newest = None;
        while s.frames.front().map(|f| f.0 <= t + 0.004).unwrap_or(false) {
            newest = s.frames.pop_front();
        }
        // nothing due yet but nothing shown either: show the first frame (e.g. a paused preview)
        if newest.is_none() && p.tex.is_none() {
            newest = s.frames.pop_front();
        }
        (newest, s.error.clone(), s.done)
    };
    if let Some((pts, img)) = newest {
        match p.tex.as_mut() {
            Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
            None => p.tex = Some(ctx.load_texture(format!("video:{key}"), img, egui::TextureOptions::LINEAR)),
        }
        p.shown = Some(pts);
    }
    match (&p.tex, error) {
        (Some(tex), _) => VideoStatus::Ready(crate::stimulus::Tex { id: tex.id(), size: tex.size_vec2() }),
        (None, Some(e)) => VideoStatus::Failed(e),
        (None, None) if done => VideoStatus::Failed("no picture".into()),
        _ => VideoStatus::Loading,
    }
}

// =============================================================================
// Video posters (first frame, small) for the Media panel
// =============================================================================
enum Poster {
    Pending(Arc<Mutex<Option<Option<ColorImage>>>>),
    Ready(TextureHandle),
    Failed,
}

static POSTERS: LazyLock<Mutex<HashMap<String, Poster>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn first_frame(path: &str, blur: bool) -> Option<ColorImage> {
    use openh264::decoder::{Decoder, DecoderConfig};
    use openh264::formats::YUVSource;
    let mut t = open_h264(path).ok()?;
    let mut dec = Decoder::with_api_config(openh264::OpenH264API::from_source(), DecoderConfig::new()).ok()?;
    let mut buf = vec![];
    for i in 1..=t.samples.min(120) {
        let Ok(Some(s)) = t.mp4.read_sample(t.track_id, i) else { continue };
        to_annex_b(&s.bytes, t.length_size, &t.sps, &t.pps, &mut buf);
        if let Ok(Some(img)) = dec.decode(&buf) {
            let (w, h) = img.dimensions();
            let mut rgba = vec![0u8; w * h * 4];
            img.write_rgba8(&mut rgba);
            let full = image::RgbaImage::from_raw(w as u32, h as u32, rgba)?;
            let mut small = image::DynamicImage::ImageRgba8(full).resize(256, 256, image::imageops::FilterType::Triangle);
            if blur {
                small = crate::stimulus::blurred(&small);
            }
            let rgba = small.to_rgba8();
            return Some(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()));
        }
    }
    None
}

/// A small picture of a video's first frame (decoded once, in the background).
pub fn poster(ctx: &egui::Context, path: &str, blur: bool) -> Option<crate::stimulus::Tex> {
    let key = format!("{}{}", file_key(path), if blur { "@blur" } else { "" });
    let Ok(mut m) = POSTERS.lock() else { return None };
    match m.get(&key) {
        Some(Poster::Ready(t)) => return Some(crate::stimulus::Tex { id: t.id(), size: t.size_vec2() }),
        Some(Poster::Failed) => return None,
        Some(Poster::Pending(slot)) => {
            let done = slot.lock().ok().and_then(|mut s| s.take());
            match done {
                Some(Some(img)) => {
                    let t = ctx.load_texture(format!("poster:{key}"), img, egui::TextureOptions::LINEAR);
                    let tex = crate::stimulus::Tex { id: t.id(), size: t.size_vec2() };
                    m.insert(key, Poster::Ready(t));
                    return Some(tex);
                }
                Some(None) => {
                    m.insert(key, Poster::Failed);
                }
                None => {}
            }
            return None;
        }
        None => {}
    }
    let slot = Arc::new(Mutex::new(None));
    let (s2, p) = (slot.clone(), path.to_string());
    let _ = std::thread::Builder::new().name("video-poster".into()).spawn(move || {
        let img = first_frame(&p, blur);
        if let Ok(mut s) = s2.lock() {
            *s = Some(img);
        }
        if let Some(c) = crate::stimulus::ctx() {
            c.request_repaint();
        }
    });
    m.insert(key, Poster::Pending(slot));
    None
}

// =============================================================================
// File info (cached) and sound preview in the editor
// =============================================================================
fn file_key(path: &str) -> String {
    let stamp = std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok())).map(|(l, t)| format!("{l}:{t:?}")).unwrap_or_default();
    format!("{path}|{stamp}")
}

static VIDEO_INFO: LazyLock<Mutex<HashMap<String, Result<VideoInfo, String>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static SOUND_INFO: LazyLock<Mutex<HashMap<String, Option<f64>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// `probe_video`, remembered per file (it reads the whole file).
pub fn video_info(path: &str) -> Result<VideoInfo, String> {
    if path.is_empty() {
        return Err("no video chosen".into());
    }
    let k = file_key(path);
    if let Some(r) = VIDEO_INFO.lock().ok().and_then(|m| m.get(&k).cloned()) {
        return r;
    }
    let r = probe_video(path);
    if let Ok(mut m) = VIDEO_INFO.lock() {
        m.insert(k, r.clone());
    }
    r
}

/// Length of a sound file (or of a video's sound) in seconds, remembered per file.
pub fn sound_duration(path: &str) -> Option<f64> {
    if path.is_empty() {
        return None;
    }
    let k = file_key(path);
    if let Some(r) = SOUND_INFO.lock().ok().and_then(|m| m.get(&k).cloned()) {
        return r;
    }
    let r = audio_duration(path).or_else(|| decode_audio(path).ok().map(|c| c.duration()));
    if let Ok(mut m) = SOUND_INFO.lock() {
        m.insert(k, r);
    }
    r
}

/// Length of a video or sound file, if it can be read.
pub fn media_duration(path: &str) -> Option<f64> {
    if is_video_file(path) {
        video_info(path).ok().map(|i| i.duration)
    } else {
        sound_duration(path)
    }
}

static PREVIEW: LazyLock<Mutex<Option<(AudioEngine, f64)>>> = LazyLock::new(|| Mutex::new(None));

/// Play a sound file once on `device` (the inspector's ▶ button). Replaces a sound still playing.
pub fn preview_sound(device: &str, path: &str, volume: f32) -> Result<(), String> {
    stop_preview();
    let clip = decode_audio(path)?;
    let until = now() + clip.duration() + 0.3;
    let e = AudioEngine::start(device)?;
    e.play("preview", clip, volume, None);
    if let Ok(mut p) = PREVIEW.lock() {
        *p = Some((e, until));
    }
    Ok(())
}

pub fn preview_playing() -> bool {
    PREVIEW.lock().map(|p| p.as_ref().map(|x| now() < x.1).unwrap_or(false)).unwrap_or(false)
}

pub fn stop_preview() {
    let old = PREVIEW.lock().ok().and_then(|mut p| p.take());
    drop(old); // closes the output
}

/// Once per frame: stop unused video decoders and close a finished sound preview.
pub fn housekeeping() {
    video_gc();
    let done = PREVIEW.lock().map(|p| p.as_ref().map(|x| now() >= x.1).unwrap_or(false)).unwrap_or(false);
    if done {
        stop_preview();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annex_b_conversion_adds_start_codes_and_parameter_sets() {
        // two NAL units with 4-byte lengths: an IDR slice (type 5) and a non-IDR slice (type 1)
        let sample = [0, 0, 0, 2, 0x65, 0xAA, 0, 0, 0, 1, 0x41];
        let mut out = vec![];
        to_annex_b(&sample, 4, &[vec![0x67, 1]], &[vec![0x68, 2]], &mut out);
        assert_eq!(out, vec![0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2, 0, 0, 0, 1, 0x65, 0xAA, 0, 0, 0, 1, 0x41]);
        // a broken length stops the conversion instead of looping forever
        to_annex_b(&[0, 0, 0, 9, 0x41], 4, &[], &[], &mut out);
        assert!(out.is_empty());
    }

    fn fixture() -> String {
        format!("{}/tests/media/tiny_h264_high.mp4", env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn video_info_and_frames_of_an_h264_high_profile_mp4() {
        let info = probe_video(&fixture()).unwrap();
        assert_eq!((info.width, info.height), (160, 96));
        assert!((info.duration - 1.0).abs() < 0.05, "{info:?}");
        // decode from the start: 10 frames, in display order, 0.1 s apart
        let s = Stream::start(&fixture(), 0.0);
        let mut got: Vec<f64> = vec![];
        let end = now() + 10.0;
        while now() < end {
            let (done, popped) = {
                let mut st = s.state.lock().unwrap();
                let popped: Vec<f64> = st.frames.drain(..).map(|f| f.0).collect();
                assert!(st.error.is_none(), "{:?}", st.error);
                (st.done, popped)
            };
            got.extend(popped);
            if done && s.state.lock().unwrap().frames.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(got.len(), 10, "{got:?}");
        for (i, t) in got.iter().enumerate() {
            assert!((t - i as f64 * 0.1).abs() < 1e-3, "{got:?}");
        }
        // seeking: frames before 0.55 s are skipped
        let s = Stream::start(&fixture(), 0.55);
        let end = now() + 10.0;
        let first = loop {
            if let Some(f) = s.state.lock().unwrap().frames.front() {
                break f.0;
            }
            assert!(now() < end, "no frame after seeking");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!((first - 0.5).abs() < 1e-3, "{first}");
    }

    #[test]
    fn the_sound_of_a_video_is_decoded_completely() {
        // (rodio's own decoder stops at the first picture packet – we pick the sound track)
        let c = decode_audio(&fixture()).unwrap();
        assert_eq!(c.rate, 22_050);
        assert!((c.duration() - 1.0).abs() < 0.1, "{}", c.duration());
        assert!(sound_duration(&fixture()).map(|d| (d - 1.0).abs() < 0.1).unwrap_or(false));
    }

    #[test]
    fn wav_files_are_decoded() {
        // 0.5 s of 16-bit mono at 8 kHz
        let dir = std::env::temp_dir().join(format!("ps_wav_{}", crate::model::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("beep.wav");
        let n = 4000u32;
        let mut w: Vec<u8> = vec![];
        w.extend(b"RIFF");
        w.extend((36 + n * 2).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes()); // PCM
        w.extend(1u16.to_le_bytes()); // mono
        w.extend(8000u32.to_le_bytes());
        w.extend((8000u32 * 2).to_le_bytes());
        w.extend(2u16.to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend((n * 2).to_le_bytes());
        for i in 0..n {
            let v = ((i as f32 / 8000.0 * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16;
            w.extend(v.to_le_bytes());
        }
        std::fs::write(&path, w).unwrap();
        let p = path.to_string_lossy().to_string();
        let c = decode_audio(&p).unwrap();
        assert_eq!((c.channels, c.rate, c.samples.len()), (1, 8000, 4000));
        assert!((media_duration(&p).unwrap() - 0.5).abs() < 1e-3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clip_source_stops_at_the_limit() {
        let c = AudioClip::tone(440.0, 1.0);
        let src = ClipSource { end: c.samples.len().min(4800), clip: c, pos: 0 };
        assert_eq!(src.count(), 4800);
    }

    #[test]
    fn tone_has_the_right_length() {
        let c = AudioClip::tone(440.0, 0.5);
        assert!((c.duration() - 0.5).abs() < 0.01);
        assert!(c.samples.iter().all(|s| s.abs() <= 0.31));
    }
}
