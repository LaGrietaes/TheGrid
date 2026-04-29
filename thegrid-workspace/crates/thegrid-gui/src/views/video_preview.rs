use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use egui::{Color32, RichText, TextureHandle, Ui};
use rodio::{Decoder, DeviceSinkBuilder, Player, Source};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoPreviewError {
    BackendMissing,
    ExtractFailed,
}

#[derive(Debug, Clone, Default)]
pub struct VideoMeta {
    pub duration_secs: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct MediaMeta {
    pub duration_secs: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
    pub has_video: bool,
    pub has_audio: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaKind {
    Video,
    Audio,
}

#[derive(Debug)]
enum AudioPreparation {
    None,
    Pending(Receiver<Result<PathBuf, String>>),
    Ready(PathBuf),
    Failed(String),
}

#[derive(Debug)]
struct VideoStreamHandle {
    frame_rx: Receiver<Result<Vec<u8>, String>>,
    stop_tx: Sender<()>,
}

pub struct MediaPreviewState {
    current_path: Option<PathBuf>,
    kind: Option<MediaKind>,
    meta: MediaMeta,
    texture: Option<TextureHandle>,
    error: Option<String>,
    note: Option<String>,
    audio_preparation: AudioPreparation,
    audio_sink: Option<rodio::MixerDeviceSink>,
    audio_player: Option<Player>,
    video_stream: Option<VideoStreamHandle>,
    volume: f32,
    muted: bool,
    playback_base: Duration,
    started_at: Option<Instant>,
    autoplay_when_ready: bool,
}

impl Default for MediaPreviewState {
    fn default() -> Self {
        Self {
            current_path: None,
            kind: None,
            meta: MediaMeta::default(),
            texture: None,
            error: None,
            note: None,
            audio_preparation: AudioPreparation::None,
            audio_sink: None,
            audio_player: None,
            video_stream: None,
            volume: 0.85,
            muted: false,
            playback_base: Duration::ZERO,
            started_at: None,
            autoplay_when_ready: false,
        }
    }
}

impl Drop for MediaPreviewState {
    fn drop(&mut self) {
        self.reset_runtime();
    }
}

pub fn is_video_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp4" | "mov" | "mkv" | "avi" | "wmv" | "webm" | "m4v" | "mpg" | "mpeg" | "mts" | "m2ts" | "mxf" | "ts"
    )
}

pub fn is_audio_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "mp3" | "wav" | "flac" | "aac" | "m4a" | "ogg" | "opus" | "aiff" | "aif" | "wma"
    )
}

pub fn backend_install_hint() -> &'static str {
    "Install ffmpeg/ffprobe and add them to PATH, or set THEGRID_FFMPEG and THEGRID_FFPROBE to the executable paths. Inline video playback and video-audio extraction use this backend."
}

pub fn extract_video_frame_png(path: &Path) -> Result<Vec<u8>, VideoPreviewError> {
    extract_video_frame_png_at(path, 1.0)
}

pub fn extract_video_frame_png_at(path: &Path, seconds: f64) -> Result<Vec<u8>, VideoPreviewError> {
    let ffmpeg = resolve_tool_path("THEGRID_FFMPEG", "ffmpeg")
        .ok_or(VideoPreviewError::BackendMissing)?;
    let output = Command::new(ffmpeg)
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg(format_seconds_for_ffmpeg(seconds))
        .arg("-i")
        .arg(path)
        .arg("-frames:v")
        .arg("1")
        .arg("-f")
        .arg("image2pipe")
        .arg("-vcodec")
        .arg("png")
        .arg("-")
        .output()
        .map_err(|_| VideoPreviewError::BackendMissing)?;

    if !output.status.success() || output.stdout.is_empty() {
        return Err(VideoPreviewError::ExtractFailed);
    }

    Ok(output.stdout)
}

pub fn probe_video_meta(path: &Path) -> Option<VideoMeta> {
    let meta = probe_media_meta(path)?;
    Some(VideoMeta {
        duration_secs: meta.duration_secs,
        width: meta.width,
        height: meta.height,
        fps: meta.fps,
    })
}

pub fn probe_media_meta(path: &Path) -> Option<MediaMeta> {
    let ffprobe = resolve_tool_path("THEGRID_FFPROBE", "ffprobe")?;
    let output = Command::new(ffprobe)
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("stream=codec_type,width,height,r_frame_rate")
        .arg("-show_entries")
        .arg("format=duration")
        .arg("-of")
        .arg("default=noprint_wrappers=1")
        .arg(path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let mut meta = MediaMeta::default();
    let mut last_codec_type = String::new();

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(v) = line.strip_prefix("codec_type=") {
            last_codec_type.clear();
            last_codec_type.push_str(v.trim());
            if last_codec_type == "video" {
                meta.has_video = true;
            } else if last_codec_type == "audio" {
                meta.has_audio = true;
            }
        } else if let Some(v) = line.strip_prefix("width=") {
            if last_codec_type == "video" {
                meta.width = v.trim().parse::<u32>().ok();
            }
        } else if let Some(v) = line.strip_prefix("height=") {
            if last_codec_type == "video" {
                meta.height = v.trim().parse::<u32>().ok();
            }
        } else if let Some(v) = line.strip_prefix("r_frame_rate=") {
            if last_codec_type == "video" {
                meta.fps = parse_ffprobe_rate(v.trim());
            }
        } else if let Some(v) = line.strip_prefix("duration=") {
            meta.duration_secs = v.trim().parse::<f64>().ok();
        }
    }

    Some(meta)
}

pub fn format_duration_short(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{:01}:{:02}:{:02}", h, m, s)
    } else {
        format!("{:02}:{:02}", m, s)
    }
}

fn parse_ffprobe_rate(rate: &str) -> Option<f32> {
    if let Some((num, den)) = rate.split_once('/') {
        let n = num.trim().parse::<f32>().ok()?;
        let d = den.trim().parse::<f32>().ok()?;
        if d > 0.0 {
            return sanitize_fps(n / d);
        }
        return None;
    }
    sanitize_fps(rate.parse::<f32>().ok()?)
}

fn sanitize_fps(fps: f32) -> Option<f32> {
    if !fps.is_finite() || fps <= 0.0 {
        return None;
    }
    if fps > 240.0 {
        return None;
    }
    Some(fps)
}

fn format_seconds_for_ffmpeg(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let whole = seconds.floor() as u64;
    let millis = ((seconds - whole as f64) * 1000.0).round() as u64;
    let h = whole / 3600;
    let m = (whole % 3600) / 60;
    let s = whole % 60;
    format!("{:02}:{:02}:{:02}.{:03}", h, m, s, millis)
}

fn rodio_duration(path: &Path) -> Option<f64> {
    let file = File::open(path).ok()?;
    let decoder = Decoder::try_from(BufReader::new(file)).ok()?;
    decoder.total_duration().map(|d| d.as_secs_f64())
}

fn load_texture_from_bytes(ctx: &egui::Context, existing: &mut Option<TextureHandle>, name: &str, bytes: &[u8]) -> Result<(), String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let size = [img.width() as usize, img.height() as usize];
    let rgba = img.to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_flat_samples().as_slice());
    if let Some(texture) = existing.as_mut() {
        texture.set(color, Default::default());
    } else {
        *existing = Some(ctx.load_texture(name.to_string(), color, Default::default()));
    }
    Ok(())
}

impl MediaPreviewState {
    fn reset_runtime(&mut self) {
        if let Some(stream) = self.video_stream.take() {
            let _ = stream.stop_tx.send(());
        }
        if let Some(player) = self.audio_player.take() {
            player.stop();
        }
        self.audio_sink = None;
        if let AudioPreparation::Ready(path) = &self.audio_preparation {
            let _ = std::fs::remove_file(path);
        }
        self.audio_preparation = AudioPreparation::None;
        self.started_at = None;
        self.autoplay_when_ready = false;
    }

    fn ensure_source(&mut self, ctx: &egui::Context, path: &Path) {
        if self.current_path.as_deref() == Some(path) {
            return;
        }

        self.reset_runtime();
        self.current_path = Some(path.to_path_buf());
        self.texture = None;
        self.error = None;
        self.note = None;
        self.playback_base = Duration::ZERO;

        let ext = path
            .extension()
            .map(|v| v.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();

        self.kind = if is_video_ext(&ext) {
            Some(MediaKind::Video)
        } else if is_audio_ext(&ext) {
            Some(MediaKind::Audio)
        } else {
            None
        };

        self.meta = probe_media_meta(path).unwrap_or_else(|| MediaMeta {
            duration_secs: if is_audio_ext(&ext) { rodio_duration(path) } else { None },
            width: None,
            height: None,
            fps: None,
            has_video: is_video_ext(&ext),
            has_audio: is_audio_ext(&ext),
        });

        if matches!(self.kind, Some(MediaKind::Video)) {
            match extract_video_frame_png_at(path, 1.0) {
                Ok(bytes) => {
                    if let Err(err) = load_texture_from_bytes(ctx, &mut self.texture, "media_preview_poster", &bytes) {
                        self.error = Some(format!("Could not decode poster frame: {}", err));
                    }
                }
                Err(VideoPreviewError::BackendMissing) => {
                    self.note = Some(backend_install_hint().to_string());
                }
                Err(VideoPreviewError::ExtractFailed) => {
                    self.error = Some("Could not extract poster frame.".to_string());
                }
            }
        }
    }

    fn current_position(&self) -> Duration {
        if let Some(player) = &self.audio_player {
            return self.playback_base.saturating_add(player.get_pos());
        }
        if let Some(started_at) = self.started_at {
            return self.playback_base.saturating_add(started_at.elapsed());
        }
        self.playback_base
    }

    fn is_playing(&self) -> bool {
        self.started_at.is_some()
    }

    fn refresh_background(&mut self, ctx: &egui::Context) {
        if let Some(player) = &self.audio_player {
            if player.empty() && self.started_at.is_some() {
                self.started_at = None;
            }
        }

        if let Some(stream) = &self.video_stream {
            let mut latest = None;
            loop {
                match stream.frame_rx.try_recv() {
                    Ok(Ok(bytes)) => latest = Some(bytes),
                    Ok(Err(err)) => {
                        self.error = Some(err);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            if let Some(bytes) = latest {
                if let Err(err) = load_texture_from_bytes(ctx, &mut self.texture, "media_preview_frame", &bytes) {
                    self.error = Some(format!("Could not decode video frame: {}", err));
                }
            }
        }

        let mut prepared_audio = None;
        let mut pending_failed = None;
        match &self.audio_preparation {
            AudioPreparation::Pending(rx) => match rx.try_recv() {
                Ok(Ok(path)) => prepared_audio = Some(path),
                Ok(Err(err)) => pending_failed = Some(err),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => pending_failed = Some("Audio preparation ended unexpectedly.".to_string()),
            },
            AudioPreparation::Failed(err) => {
                self.note = Some(err.clone());
            }
            _ => {}
        }
        if let Some(path) = prepared_audio {
            self.audio_preparation = AudioPreparation::Ready(path);
            if self.autoplay_when_ready {
                self.autoplay_when_ready = false;
                if let Err(err) = self.start_prepared_audio() {
                    self.note = Some(err);
                }
            }
        }
        if let Some(err) = pending_failed {
            self.audio_preparation = AudioPreparation::Failed(err.clone());
            self.note = Some(err);
            self.autoplay_when_ready = false;
        }

        if self.is_playing() {
            ctx.request_repaint();
        }
    }

    fn pause_playback(&mut self) {
        self.playback_base = self.current_position();
        if let Some(stream) = self.video_stream.take() {
            let _ = stream.stop_tx.send(());
        }
        if let Some(player) = self.audio_player.take() {
            player.stop();
        }
        self.audio_sink = None;
        if let AudioPreparation::Ready(path) = &self.audio_preparation {
            let _ = std::fs::remove_file(path);
            self.audio_preparation = AudioPreparation::None;
        }
        self.started_at = None;
        self.autoplay_when_ready = false;
    }

    fn stop_playback(&mut self, ctx: &egui::Context) {
        self.pause_playback();
        self.playback_base = Duration::ZERO;
        if let Some(path) = self.current_path.clone() {
            self.ensure_source(ctx, &path);
        }
    }

    fn seek_fraction(&mut self, ctx: &egui::Context, fraction: f32) {
        let Some(duration_secs) = self.meta.duration_secs else { return; };
        let target = Duration::from_secs_f64(duration_secs.max(0.0) * fraction.clamp(0.0, 1.0) as f64);
        let was_playing = self.is_playing();
        self.pause_playback();
        self.playback_base = target;
        if matches!(self.kind, Some(MediaKind::Video)) {
            if let Some(path) = self.current_path.as_deref() {
                match extract_video_frame_png_at(path, target.as_secs_f64()) {
                    Ok(bytes) => {
                        let _ = load_texture_from_bytes(ctx, &mut self.texture, "media_preview_seek", &bytes);
                    }
                    Err(VideoPreviewError::BackendMissing) => {
                        self.note = Some(backend_install_hint().to_string());
                    }
                    Err(VideoPreviewError::ExtractFailed) => {
                        self.error = Some("Could not seek preview frame.".to_string());
                    }
                }
            }
        }
        if was_playing {
            let _ = self.start_playback(ctx);
        }
    }

    fn start_playback(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(path) = self.current_path.clone() else {
            return Ok(());
        };

        self.pause_playback();
        self.error = None;
        self.note = None;

        let offset = self.playback_base;
        let direct_audio = self.try_start_direct_audio(&path, offset);
        let audio_started = direct_audio.is_ok();

        if let Err(err) = direct_audio {
            let needs_audio = self.meta.has_audio || matches!(self.kind, Some(MediaKind::Audio));
            if needs_audio {
                if resolve_tool_path("THEGRID_FFMPEG", "ffmpeg").is_some() {
                    self.spawn_audio_prepare(&path, offset);
                    self.autoplay_when_ready = true;
                } else if matches!(self.kind, Some(MediaKind::Audio)) {
                    self.note = Some(format!("{} Audio preview is unavailable for this codec without ffmpeg.", err));
                } else {
                    self.note = Some(backend_install_hint().to_string());
                }
            }
        }

        if matches!(self.kind, Some(MediaKind::Video)) {
            self.start_video_stream(ctx)?;
        }

        if audio_started || matches!(self.kind, Some(MediaKind::Video)) {
            self.started_at = Some(Instant::now());
        }

        Ok(())
    }

    fn start_video_stream(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let Some(path) = self.current_path.clone() else {
            return Ok(());
        };
        let ffmpeg = resolve_tool_path("THEGRID_FFMPEG", "ffmpeg")
            .ok_or_else(|| backend_install_hint().to_string())?;

        let fps = self.meta.fps.unwrap_or(24.0).clamp(10.0, 30.0);
        let vf = format!("fps={:.2},scale='min(1280,iw)':-2:flags=lanczos", fps);
        let (frame_tx, frame_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let seek = format_seconds_for_ffmpeg(self.playback_base.as_secs_f64());

        std::thread::spawn(move || {
            let mut child = match Command::new(ffmpeg)
                .arg("-hide_banner")
                .arg("-loglevel")
                .arg("error")
                .arg("-fflags")
                .arg("nobuffer")
                .arg("-flags")
                .arg("low_delay")
                .arg("-ss")
                .arg(seek)
                .arg("-re")
                .arg("-i")
                .arg(&path)
                .arg("-an")
                .arg("-vf")
                .arg(vf)
                .arg("-f")
                .arg("image2pipe")
                .arg("-vcodec")
                .arg("mjpeg")
                .arg("-q:v")
                .arg("3")
                .arg("-")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn() {
                    Ok(child) => child,
                    Err(err) => {
                        let _ = frame_tx.send(Err(format!("Could not start ffmpeg video pipeline: {}", err)));
                        return;
                    }
                };

            let mut stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => {
                    let _ = frame_tx.send(Err("Could not read ffmpeg video output.".to_string()));
                    let _ = child.kill();
                    return;
                }
            };

            let mut pending = Vec::new();
            let mut chunk = [0_u8; 32 * 1024];

            loop {
                if stop_rx.try_recv().is_ok() {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }

                match stdout.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        pending.extend_from_slice(&chunk[..read]);
                        while let Some(frame) = extract_jpeg_frame(&mut pending) {
                            if frame_tx.send(Ok(frame)).is_err() {
                                let _ = child.kill();
                                let _ = child.wait();
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        let _ = frame_tx.send(Err(format!("Video stream read failed: {}", err)));
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                }
            }

            let mut stderr = String::new();
            if let Some(mut err_stream) = child.stderr.take() {
                let _ = err_stream.read_to_string(&mut stderr);
            }
            let _ = child.wait();
            if !stderr.trim().is_empty() {
                let _ = frame_tx.send(Err(stderr.trim().to_string()));
            }
        });

        self.video_stream = Some(VideoStreamHandle { frame_rx, stop_tx });
        ctx.request_repaint();
        Ok(())
    }

    fn try_start_direct_audio(&mut self, path: &Path, seek_offset: Duration) -> Result<(), String> {
        let mut sink = DeviceSinkBuilder::open_default_sink().map_err(|e| e.to_string())?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        let file = File::open(path).map_err(|e| e.to_string())?;
        let decoder = Decoder::try_from(BufReader::new(file)).map_err(|e| e.to_string())?;
        player.append(decoder);
        if seek_offset > Duration::ZERO {
            let _ = player.try_seek(seek_offset);
        }
        player.set_volume(if self.muted { 0.0 } else { self.volume });
        player.play();
        self.audio_sink = Some(sink);
        self.audio_player = Some(player);
        Ok(())
    }

    fn spawn_audio_prepare(&mut self, path: &Path, seek_offset: Duration) {
        let Some(ffmpeg) = resolve_tool_path("THEGRID_FFMPEG", "ffmpeg") else {
            self.note = Some(backend_install_hint().to_string());
            return;
        };

        let input = path.to_path_buf();
        let output = std::env::temp_dir().join(format!("thegrid_preview_{}.wav", Uuid::new_v4()));
        let seek = format_seconds_for_ffmpeg(seek_offset.as_secs_f64());
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let status = Command::new(ffmpeg)
                .arg("-hide_banner")
                .arg("-loglevel")
                .arg("error")
                .arg("-ss")
                .arg(seek)
                .arg("-i")
                .arg(&input)
                .arg("-map")
                .arg("0:a:0")
                .arg("-vn")
                .arg("-ac")
                .arg("2")
                .arg("-ar")
                .arg("44100")
                .arg("-f")
                .arg("wav")
                .arg("-y")
                .arg(&output)
                .status();

            match status {
                Ok(status) if status.success() => {
                    let _ = tx.send(Ok(output));
                }
                Ok(_) => {
                    let _ = tx.send(Err("ffmpeg could not decode an audio preview track.".to_string()));
                }
                Err(err) => {
                    let _ = tx.send(Err(format!("Could not start ffmpeg audio extraction: {}", err)));
                }
            }
        });

        self.audio_preparation = AudioPreparation::Pending(rx);
    }

    fn start_prepared_audio(&mut self) -> Result<(), String> {
        let AudioPreparation::Ready(path) = &self.audio_preparation else {
            return Ok(());
        };
        let mut sink = DeviceSinkBuilder::open_default_sink().map_err(|e| e.to_string())?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        let file = File::open(path).map_err(|e| e.to_string())?;
        let decoder = Decoder::try_from(BufReader::new(file)).map_err(|e| e.to_string())?;
        player.append(decoder);
        player.set_volume(if self.muted { 0.0 } else { self.volume });
        player.play();
        self.audio_sink = Some(sink);
        self.audio_player = Some(player);
        Ok(())
    }

    fn format_summary(&self) -> Option<String> {
        let duration = self.meta.duration_secs.map(format_duration_short);
        let resolution = match (self.meta.width, self.meta.height) {
            (Some(w), Some(h)) => Some(format!("{}x{}", w, h)),
            _ => None,
        };
        let fps = self.meta.fps.map(|v| format!("{:.1}fps", v));

        let mut parts = Vec::new();
        if let Some(duration) = duration {
            parts.push(duration);
        }
        if let Some(resolution) = resolution {
            parts.push(resolution);
        }
        if let Some(fps) = fps {
            parts.push(fps);
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("  |  "))
        }
    }
}

pub fn render_media_preview(ui: &mut Ui, state: &mut MediaPreviewState, path: &Path, min_height: f32) -> bool {
    let ext = path
        .extension()
        .map(|v| v.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    if !is_video_ext(&ext) && !is_audio_ext(&ext) {
        return false;
    }

    state.ensure_source(ui.ctx(), path);
    state.refresh_background(ui.ctx());

    if matches!(state.audio_preparation, AudioPreparation::Ready(_)) && state.audio_player.is_none() && state.autoplay_when_ready {
        state.autoplay_when_ready = false;
        if let Err(err) = state.start_prepared_audio() {
            state.note = Some(err);
        } else {
            if matches!(state.kind, Some(MediaKind::Video)) {
                let _ = state.start_video_stream(ui.ctx());
            }
            state.started_at = Some(Instant::now());
        }
    }

    if let Some(summary) = state.format_summary() {
        ui.label(RichText::new(summary).size(8.5).color(Color32::GRAY));
        ui.add_space(6.0);
    }

    let kind = state.kind;
    egui::Frame::none()
        .fill(Color32::from_rgb(6, 8, 9))
        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(42, 48, 52)))
        .inner_margin(egui::Margin::same(6.0))
        .show(ui, |ui| {
            ui.set_min_height(min_height);
            match kind {
                Some(MediaKind::Video) => {
                    if let Some(texture) = &state.texture {
                        let avail = ui.available_width() - 8.0;
                        ui.centered_and_justified(|ui| {
                            ui.add(egui::Image::from_texture(texture)
                                .max_width(avail)
                                .maintain_aspect_ratio(true));
                        });
                    } else if matches!(state.audio_preparation, AudioPreparation::Pending(_)) {
                        ui.centered_and_justified(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.spinner();
                                ui.add_space(6.0);
                                ui.label(RichText::new("Preparing preview...").color(Color32::GRAY).size(8.5));
                            });
                        });
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label(RichText::new("Preview frame unavailable").color(Color32::GRAY).size(8.5));
                        });
                    }
                }
                Some(MediaKind::Audio) => {
                    ui.centered_and_justified(|ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("AUDIO PREVIEW").strong().size(10.0));
                            ui.add_space(6.0);
                            ui.label(RichText::new("Play, pause, seek, and listen inline while sorting.").color(Color32::GRAY).size(8.5));
                        });
                    });
                }
                None => {}
            }
        });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let play_label = if state.is_playing() { "PAUSE" } else if state.current_position() > Duration::ZERO { "RESUME" } else { "PLAY" };
        if ui.button(play_label).clicked() {
            if state.is_playing() {
                state.pause_playback();
            } else if let Err(err) = state.start_playback(ui.ctx()) {
                state.error = Some(err);
            }
        }
        if ui.button("STOP").clicked() {
            state.stop_playback(ui.ctx());
        }

        let mut volume = state.volume;
        if ui.add(egui::Slider::new(&mut volume, 0.0..=1.5).show_value(false).text("VOL")).changed() {
            state.volume = volume;
            if let Some(player) = &state.audio_player {
                player.set_volume(if state.muted { 0.0 } else { state.volume });
            }
        }
        if ui.selectable_label(state.muted, "MUTE").clicked() {
            state.muted = !state.muted;
            if let Some(player) = &state.audio_player {
                player.set_volume(if state.muted { 0.0 } else { state.volume });
            }
        }
    });

    if let Some(duration_secs) = state.meta.duration_secs {
        let mut frac = (state.current_position().as_secs_f64() / duration_secs.max(0.001)) as f32;
        frac = frac.clamp(0.0, 1.0);
        if ui.add(egui::Slider::new(&mut frac, 0.0..=1.0).show_value(false)).changed() {
            state.seek_fraction(ui.ctx(), frac);
        }
        ui.label(
            RichText::new(format!(
                "{} / {}",
                format_duration_short(state.current_position().as_secs_f64()),
                format_duration_short(duration_secs)
            ))
            .size(8.0)
            .color(Color32::GRAY),
        );
    }

    if matches!(state.audio_preparation, AudioPreparation::Pending(_)) {
        ui.label(RichText::new("Preparing audio preview...").size(8.0).color(Color32::GRAY));
    }
    if let Some(note) = &state.note {
        ui.label(RichText::new(note).size(8.0).color(Color32::GRAY));
    }
    if let Some(err) = &state.error {
        ui.label(RichText::new(err).size(8.0).color(Color32::RED));
    }

    true
}

fn extract_jpeg_frame(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let start = buffer.windows(2).position(|w| w == [0xFF, 0xD8])?;
    let end_rel = buffer[start + 2..]
        .windows(2)
        .position(|w| w == [0xFF, 0xD9])?;
    let end = start + 2 + end_rel + 2;
    let frame = buffer[start..end].to_vec();
    buffer.drain(..end);
    Some(frame)
}

fn resolve_tool_path(env_var: &str, exe_stem: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(env_var) {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if command_exists(exe_stem) {
        return Some(PathBuf::from(exe_stem));
    }

    for candidate in common_windows_paths(exe_stem) {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn command_exists(exe_stem: &str) -> bool {
    Command::new(exe_stem)
        .arg("-version")
        .output()
        .is_ok()
}

fn common_windows_paths(exe_stem: &str) -> Vec<PathBuf> {
    let exe_name = format!("{}.exe", exe_stem);
    let mut paths = Vec::new();

    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        paths.push(PathBuf::from(&program_files).join("ffmpeg").join("bin").join(&exe_name));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        paths.push(PathBuf::from(&program_files_x86).join("ffmpeg").join("bin").join(&exe_name));
    }
    if let Some(user_profile) = std::env::var_os("USERPROFILE") {
        paths.push(PathBuf::from(&user_profile).join("scoop").join("apps").join("ffmpeg").join("current").join("bin").join(&exe_name));
    }
    if let Some(choco) = std::env::var_os("ChocolateyInstall") {
        paths.push(PathBuf::from(&choco).join("bin").join(&exe_name));
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        let winget_root = PathBuf::from(&local_app_data).join("Microsoft").join("WinGet");
        paths.push(winget_root.join("Links").join(&exe_name));
        paths.extend(find_winget_package_binaries(&winget_root.join("Packages"), &exe_name));
    }

    paths
}

fn find_winget_package_binaries(packages_root: &Path, exe_name: &str) -> Vec<PathBuf> {
    let mut matches = Vec::new();
    let Ok(package_dirs) = std::fs::read_dir(packages_root) else {
        return matches;
    };

    for package_dir in package_dirs.flatten() {
        let package_name = package_dir.file_name().to_string_lossy().to_ascii_lowercase();
        if !package_name.contains("ffmpeg") {
            continue;
        }

        let package_path = package_dir.path();
        let Ok(first_level_dirs) = std::fs::read_dir(&package_path) else {
            continue;
        };

        for first_level in first_level_dirs.flatten() {
            let first_level_path = first_level.path();
            if first_level_path.is_file() && first_level_path.file_name().is_some_and(|name| name == exe_name) {
                matches.push(first_level_path);
                continue;
            }

            let bin_candidate = first_level_path.join("bin").join(exe_name);
            if bin_candidate.is_file() {
                matches.push(bin_candidate);
                continue;
            }

            let Ok(second_level_dirs) = std::fs::read_dir(&first_level_path) else {
                continue;
            };

            for second_level in second_level_dirs.flatten() {
                let second_level_path = second_level.path();
                let nested_bin = second_level_path.join("bin").join(exe_name);
                if nested_bin.is_file() {
                    matches.push(nested_bin);
                }
            }
        }
    }

    matches
}