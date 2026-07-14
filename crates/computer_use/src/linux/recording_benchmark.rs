use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use instant::Instant;
use tokio::process::{Child, Command};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    self, ConnectionExt as _, CreateGCAux, CreateWindowAux, Rectangle, WindowClass,
};
use x11rb::rust_connection::RustConnection;

use super::Recorder;
use crate::{Recorder as _, RecordingConfig, Target};

const TARGET_PIXEL: u32 = 0x00FF_0000;
const COVER_PIXEL: u32 = 0x0000_00FF;
const ACCENT_PIXEL: u32 = 0x00FF_FFFF;

#[derive(Clone, Copy)]
enum CaptureCase {
    Screen,
    CompositeVisible,
    CompositeCovered,
    NativeWindowVisible,
    NativeWindowCovered,
}

impl CaptureCase {
    const ALL: [Self; 5] = [
        Self::Screen,
        Self::CompositeVisible,
        Self::CompositeCovered,
        Self::NativeWindowVisible,
        Self::NativeWindowCovered,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Screen => "screen_x11grab",
            Self::CompositeVisible => "composite_visible",
            Self::CompositeCovered => "composite_covered",
            Self::NativeWindowVisible => "native_window_visible",
            Self::NativeWindowCovered => "native_window_covered",
        }
    }

    fn is_covered(self) -> bool {
        matches!(self, Self::CompositeCovered | Self::NativeWindowCovered)
    }

    fn uses_native_window(self) -> bool {
        matches!(self, Self::NativeWindowVisible | Self::NativeWindowCovered)
    }

    fn target(self, window: xproto::Window) -> Target {
        match self {
            Self::Screen => Target::Screen,
            Self::CompositeVisible | Self::CompositeCovered => Target::Window {
                window_id: window,
                pid: 0,
            },
            Self::NativeWindowVisible | Self::NativeWindowCovered => unreachable!(),
        }
    }

    fn expected_sample(self) -> Option<&'static str> {
        match self {
            Self::NativeWindowCovered => None,
            _ => Some("target"),
        }
    }
}

struct BenchmarkRow {
    case: &'static str,
    repetition: usize,
    frame_rate: u32,
    width: u32,
    height: u32,
    start_ms: f64,
    capture_wall_ms: f64,
    stop_ms: f64,
    media_duration_s: f64,
    frames: u64,
    wall_fps: f64,
    size_bytes: u64,
    sample: &'static str,
}

struct ProbeResult {
    duration_s: f64,
    frames: u64,
}
#[derive(Clone, Copy)]
struct BenchmarkParameters {
    duration: Duration,
    width: u16,
    height: u16,
}

struct NativeCapture {
    process: Child,
    path: PathBuf,
}

struct WindowPainter<'a> {
    conn: &'a RustConnection,
    window: xproto::Window,
    width: u16,
    height: u16,
    background_gc: xproto::Gcontext,
    accent_gc: xproto::Gcontext,
}

impl<'a> WindowPainter<'a> {
    fn new(conn: &'a RustConnection, window: xproto::Window, width: u16, height: u16) -> Self {
        let background_gc = conn.generate_id().expect("generate background gc");
        conn.create_gc(
            background_gc,
            window,
            &CreateGCAux::new().foreground(TARGET_PIXEL),
        )
        .expect("create background gc")
        .check()
        .expect("check background gc");
        let accent_gc = conn.generate_id().expect("generate accent gc");
        conn.create_gc(
            accent_gc,
            window,
            &CreateGCAux::new().foreground(ACCENT_PIXEL),
        )
        .expect("create accent gc")
        .check()
        .expect("check accent gc");
        Self {
            conn,
            window,
            width,
            height,
            background_gc,
            accent_gc,
        }
    }

    fn paint(&self, frame: u32) {
        self.conn
            .poly_fill_rectangle(
                self.window,
                self.background_gc,
                &[Rectangle {
                    x: 0,
                    y: 0,
                    width: self.width,
                    height: self.height,
                }],
            )
            .expect("paint background");
        let square = self.width.min(self.height).clamp(32, 160);
        let x_range = self.width.saturating_sub(square).max(1);
        let x = ((frame * 31) % u32::from(x_range)) as i16;
        self.conn
            .poly_fill_rectangle(
                self.window,
                self.accent_gc,
                &[Rectangle {
                    x,
                    y: (self.height / 3) as i16,
                    width: square,
                    height: square,
                }],
            )
            .expect("paint accent");
        self.conn.flush().expect("flush animation");
    }
}

impl Drop for WindowPainter<'_> {
    fn drop(&mut self) {
        let _ = self.conn.free_gc(self.background_gc);
        let _ = self.conn.free_gc(self.accent_gc);
        let _ = self.conn.flush();
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "performance benchmark; run with script/benchmark_linux_recording"]
async fn benchmark_recording_capture_paths() {
    assert_benchmark_environment().await;

    let duration = Duration::from_secs(env_parse("WARP_RECORDING_BENCHMARK_SECONDS", 3));
    let repetitions = env_parse("WARP_RECORDING_BENCHMARK_REPETITIONS", 3);
    let frame_rates = frame_rates();
    let output_path = std::env::var_os("WARP_RECORDING_BENCHMARK_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("recording-benchmark.csv"));

    let (conn, screen_index) = RustConnection::connect(None).expect("connect to X11");
    let screen = conn.setup().roots[screen_index].clone();
    let width = screen.width_in_pixels & !1;
    let height = screen.height_in_pixels & !1;
    assert!(width > 0 && height > 0, "X11 screen must be non-empty");
    let parameters = BenchmarkParameters {
        duration,
        width,
        height,
    };

    let mut rows = Vec::new();
    for frame_rate in frame_rates {
        for repetition in 1..=repetitions {
            for case in CaptureCase::ALL {
                let row = run_case(&conn, &screen, case, repetition, frame_rate, parameters).await;
                eprintln!(
                    "{} rep={} fps={} start={:.1}ms stop={:.1}ms frames={} wall_fps={:.2} sample={}",
                    row.case,
                    row.repetition,
                    row.frame_rate,
                    row.start_ms,
                    row.stop_ms,
                    row.frames,
                    row.wall_fps,
                    row.sample,
                );
                rows.push(row);
            }
        }
    }

    write_csv(&output_path, &rows);
    eprintln!("benchmark results: {}", output_path.display());
}

async fn run_case(
    conn: &RustConnection,
    screen: &xproto::Screen,
    case: CaptureCase,
    repetition: usize,
    frame_rate: u32,
    parameters: BenchmarkParameters,
) -> BenchmarkRow {
    let BenchmarkParameters {
        duration,
        width,
        height,
    } = parameters;
    let target = create_window(conn, screen, width, height, TARGET_PIXEL);
    let painter = WindowPainter::new(conn, target, width, height);
    painter.paint(0);
    let cover = case
        .is_covered()
        .then(|| create_window(conn, screen, width, height, COVER_PIXEL));
    conn.flush().expect("flush benchmark windows");

    let start = Instant::now();
    let (path, start_ms, capture_wall_ms, stop_ms, size_bytes) = if case.uses_native_window() {
        let mut capture = start_native_window_capture(target, width, height, frame_rate)
            .await
            .expect("start native window capture");
        let start_ms = start.elapsed().as_secs_f64() * 1000.0;
        let capture_start = Instant::now();
        animate(&painter, duration).await;
        let capture_wall_ms = capture_start.elapsed().as_secs_f64() * 1000.0;
        let stop_start = Instant::now();
        super::finalize_screen_capture(&mut capture.process, &capture.path)
            .await
            .expect("finalize native window capture");
        let stop_ms = stop_start.elapsed().as_secs_f64() * 1000.0;
        let size_bytes = std::fs::metadata(&capture.path)
            .expect("native recording metadata")
            .len();
        (capture.path, start_ms, capture_wall_ms, stop_ms, size_bytes)
    } else {
        let recorder = Recorder::new();
        let config = RecordingConfig {
            frame_rate,
            target: case.target(target),
            max_duration: duration + Duration::from_secs(30),
            ..RecordingConfig::default()
        };
        let handle = recorder.start(config).await.expect("start recorder");
        let start_ms = start.elapsed().as_secs_f64() * 1000.0;
        let capture_start = Instant::now();
        animate(&painter, duration).await;
        let capture_wall_ms = capture_start.elapsed().as_secs_f64() * 1000.0;
        let stop_start = Instant::now();
        let output = recorder.stop(handle).await.expect("stop recorder");
        let stop_ms = stop_start.elapsed().as_secs_f64() * 1000.0;
        (
            output.path,
            start_ms,
            capture_wall_ms,
            stop_ms,
            output.size_bytes,
        )
    };

    let probe = probe_video(&path).await;
    let sample = sample_recording(&path, u32::from(width), u32::from(height)).await;
    if let Some(expected) = case.expected_sample() {
        assert_eq!(sample, expected, "{} captured wrong pixels", case.name());
    }
    let wall_fps = probe.frames as f64 / (capture_wall_ms / 1000.0);

    let _ = std::fs::remove_file(&path);
    if let Some(cover) = cover {
        let _ = conn.destroy_window(cover);
    }
    let _ = conn.destroy_window(target);
    let _ = conn.flush();

    BenchmarkRow {
        case: case.name(),
        repetition,
        frame_rate,
        width: u32::from(width),
        height: u32::from(height),
        start_ms,
        capture_wall_ms,
        stop_ms,
        media_duration_s: probe.duration_s,
        frames: probe.frames,
        wall_fps,
        size_bytes,
        sample,
    }
}

async fn animate(painter: &WindowPainter<'_>, duration: Duration) {
    let deadline = Instant::now() + duration;
    let mut frame = 0;
    while Instant::now() < deadline {
        painter.paint(frame);
        frame += 1;
        tokio::time::sleep(Duration::from_millis(16)).await;
    }
}

fn create_window(
    conn: &RustConnection,
    screen: &xproto::Screen,
    width: u16,
    height: u16,
    color: u32,
) -> xproto::Window {
    let window = conn.generate_id().expect("generate window id");
    conn.create_window(
        screen.root_depth,
        window,
        screen.root,
        0,
        0,
        width,
        height,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new().background_pixel(color),
    )
    .expect("create benchmark window")
    .check()
    .expect("check benchmark window");
    conn.map_window(window).expect("map benchmark window");
    conn.flush().expect("flush benchmark window");
    window
}

async fn start_native_window_capture(
    window: xproto::Window,
    width: u16,
    height: u16,
    frame_rate: u32,
) -> Result<NativeCapture, String> {
    let display = std::env::var("DISPLAY").map_err(|_| "DISPLAY is not set".to_string())?;
    let path = std::env::temp_dir().join(format!(
        "warp-recording-benchmark-{}.mp4",
        uuid::Uuid::new_v4()
    ));
    let log_path = path.with_extension("log");
    let log = File::create(&log_path).map_err(|e| e.to_string())?;
    let mut process = Command::new("ffmpeg")
        .arg("-y")
        .args(["-f", "x11grab"])
        .args(["-framerate", &frame_rate.to_string()])
        .args(["-window_id", &window.to_string()])
        .args(["-draw_mouse", "0"])
        .args(["-video_size", &format!("{width}x{height}")])
        .args(["-i", &display])
        .args(["-c:v", "libx264"])
        .args(["-preset", "ultrafast"])
        .args(["-pix_fmt", "yuv420p"])
        .args(["-movflags", "+faststart"])
        .arg(&path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Err(error) = super::wait_for_first_output(&path, &mut process).await {
        let _ = process.start_kill();
        let detail = std::fs::read_to_string(&log_path).unwrap_or_default();
        return Err(format!("{error}: {detail}"));
    }
    let _ = std::fs::remove_file(log_path);
    Ok(NativeCapture { process, path })
}

async fn probe_video(path: &Path) -> ProbeResult {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args([
            "-map",
            "0:v:0",
            "-f",
            "null",
            "-",
            "-progress",
            "pipe:1",
            "-nostats",
        ])
        .output()
        .await
        .expect("probe recording");
    assert!(
        output.status.success(),
        "failed to probe {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr),
    );
    let progress = String::from_utf8_lossy(&output.stdout);
    let frames = progress
        .lines()
        .rev()
        .filter_map(|line| line.strip_prefix("frame="))
        .find_map(|value| value.parse().ok())
        .expect("ffmpeg progress frame count");
    let duration_s = progress
        .lines()
        .rev()
        .filter_map(|line| line.strip_prefix("out_time="))
        .find_map(parse_timecode)
        .expect("ffmpeg progress duration");
    ProbeResult { duration_s, frames }
}

fn parse_timecode(value: &str) -> Option<f64> {
    let mut parts = value.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds)
}

async fn sample_recording(path: &Path, width: u32, height: u32) -> &'static str {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-sseof", "-0.1", "-i"])
        .arg(path)
        .args([
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "pipe:1",
        ])
        .output()
        .await
        .expect("decode sample frame");
    assert!(output.status.success(), "decode sample frame failed");
    let x = width.saturating_sub(16);
    let y = height.saturating_sub(16);
    let offset = ((y * width + x) * 3) as usize;
    let [r, g, b] = output.stdout[offset..offset + 3] else {
        panic!("decoded frame was smaller than expected");
    };
    let (r, g, b) = (i16::from(r), i16::from(g), i16::from(b));
    if r > b + 40 && r > g + 40 {
        "target"
    } else if b > r + 40 && b > g + 40 {
        "cover"
    } else {
        "other"
    }
}

async fn assert_benchmark_environment() {
    assert!(
        std::env::var_os("DISPLAY").is_some(),
        "DISPLAY must point to Xvfb"
    );
    for command in ["ffmpeg"] {
        let status = Command::new(command)
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .unwrap_or_else(|_| panic!("{command} is required"));
        assert!(status.success(), "{command} is required");
    }
    RustConnection::connect(None).expect("DISPLAY must accept X11 connections");
}

fn frame_rates() -> Vec<u32> {
    std::env::var("WARP_RECORDING_BENCHMARK_FPS")
        .unwrap_or_else(|_| "15,30".to_string())
        .split(',')
        .map(|value| value.trim().parse().expect("invalid benchmark FPS"))
        .collect()
}

fn env_parse<T>(name: &str, default: T) -> T
where
    T: std::str::FromStr,
    T::Err: std::fmt::Debug,
{
    std::env::var(name)
        .map(|value| value.parse().expect("invalid benchmark setting"))
        .unwrap_or(default)
}

fn write_csv(path: &Path, rows: &[BenchmarkRow]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create benchmark output directory");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .expect("create benchmark output");
    writeln!(
        file,
        "case,repetition,requested_fps,width,height,start_ms,capture_wall_ms,stop_ms,media_duration_s,frames,wall_fps,size_bytes,sample"
    )
    .expect("write benchmark header");
    for row in rows {
        writeln!(
            file,
            "{},{},{},{},{},{:.3},{:.3},{:.3},{:.6},{},{:.3},{},{}",
            row.case,
            row.repetition,
            row.frame_rate,
            row.width,
            row.height,
            row.start_ms,
            row.capture_wall_ms,
            row.stop_ms,
            row.media_duration_s,
            row.frames,
            row.wall_fps,
            row.size_bytes,
            row.sample,
        )
        .expect("write benchmark row");
    }
}
