//! A film kept on its side, as a phone keeps one shot upright, with a
//! display matrix saying how it is shown, is handed over upright: its
//! frames at the size it is shown at, each corner where a viewer shows it
//! (ffmpeg, which draws the thumbnails and the poster, turns it so), from
//! the first frame, after a seek, after a track change and while it plays.

// Tests run from a console: no console window to keep away.
#![allow(clippy::disallowed_methods)]

use skwir_core::media::{Control, MAX_MESSAGE, PIXELS_FILE, Reply, Request};
use std::{
    io::{BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

/// The clip as stored: 320 by 160, red, its top-left corner blue and its
/// top-right lime, with a second of sound (a track to change).
fn stored(dir: &Path) -> PathBuf {
    let clip = dir.join("stored.mov");
    ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "color=c=red:size=320x160:rate=10,\
             drawbox=x=0:y=0:w=80:h=40:color=blue:t=fill,\
             drawbox=x=240:y=0:w=80:h=40:color=lime:t=fill",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "2",
            "-c:v",
            "mjpeg",
            "-q:v",
            "2",
            "-c:a",
            "pcm_s16le",
        ],
        &clip,
    );
    clip
}

/// `clip` with a display matrix: turned `degrees` counterclockwise, as
/// ffmpeg counts them (a phone held upright: -90), and mirrored first
/// where `mirrored`.
fn turned(clip: &Path, degrees: i32, mirrored: bool) -> PathBuf {
    let out = clip.with_file_name(format!(
        "A phone's film {degrees}{}.mov",
        if mirrored { " mirrored" } else { "" }
    ));
    let degrees = degrees.to_string();
    let mut args = vec!["-display_rotation", degrees.as_str()];
    if mirrored {
        args.push("-display_hflip");
    }
    let clip = clip.to_str().unwrap();
    args.extend(["-i", clip, "-c", "copy"]);
    ffmpeg(&args, &out);
    out
}

fn ffmpeg(args: &[&str], out: &Path) {
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y"])
        .args(args)
        .arg(out)
        .status()
        .expect("ffmpeg makes the test's clips");
    assert!(status.success(), "ffmpeg failed on {}", out.display());
}

/// The helper playing `clip`, its replies read on a thread of their own.
struct Worker {
    child: Child,
    input: Option<ChildStdin>,
    replies: mpsc::Receiver<Reply>,
    slot: tempfile::TempDir,
    generation: u64,
}

impl Worker {
    fn open(clip: &Path) -> Self {
        let slot = tempfile::tempdir().unwrap();
        std::fs::File::create(slot.path().join(PIXELS_FILE)).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_skwir-media-worker"))
            .arg(clip)
            .arg(slot.path())
            // A clocked fake sound output, never the desktop's.
            .env("SKWIR_MEDIA_TEST_AUDIO", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let (tx, replies) = mpsc::channel();
        std::thread::spawn(move || {
            while let Some(Ok(reply)) = skwir_core::jsonl::read::<Reply>(&mut output, MAX_MESSAGE) {
                if tx.send(reply).is_err() {
                    break;
                }
            }
        });
        Self {
            input: child.stdin.take(),
            child,
            replies,
            slot,
            generation: 0,
        }
    }

    fn send(&mut self, request: &Request) {
        let line = skwir_core::jsonl::encode(request).unwrap();
        let input = self.input.as_mut().unwrap();
        input.write_all(&line).unwrap();
        input.flush().unwrap();
    }

    fn control(&mut self, command: Control) {
        self.generation += 1;
        let generation = self.generation;
        self.send(&Request::Control {
            generation,
            command,
        });
    }

    /// The next frame of the latest control: its size, and the class of
    /// the colour at each corner (top-left, top-right, bottom-left,
    /// bottom-right). Each frame is taken, as the window takes it.
    fn frame(&mut self) -> ((u32, u32), String) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let reply = self.replies.recv_timeout(left).expect("a frame in time");
            match reply {
                Reply::Frame {
                    generation,
                    width,
                    height,
                    ..
                } => {
                    let bytes = std::fs::read(self.slot.path().join(PIXELS_FILE)).unwrap();
                    let size = (width as usize, height as usize);
                    let pixels = &bytes[..size.0 * size.1 * 4];
                    self.send(&Request::Consumed);
                    if generation == self.generation {
                        return ((width, height), corners(pixels, size));
                    }
                }
                Reply::Error(error) | Reply::Unavailable(error) => panic!("{error}"),
                Reply::Rejected { message, .. } => panic!("{message}"),
                _ => {}
            }
        }
    }
}

impl Drop for Worker {
    /// Ended as the manager ends it.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The colour at each corner of a BGRA picture, a few pixels in: `B`lue,
/// `G`reen (lime), `R`ed, or `?`.
fn corners(bgra: &[u8], (width, height): (usize, usize)) -> String {
    [
        (2, 2),
        (width - 3, 2),
        (2, height - 3),
        (width - 3, height - 3),
    ]
    .into_iter()
    .map(|(x, y)| {
        let at = (y * width + x) * 4;
        match (bgra[at + 2], bgra[at + 1], bgra[at]) {
            (r, g, b) if r > 150 && g < 100 && b < 100 => 'R',
            (r, g, b) if g > 150 && r < 100 && b < 100 => 'G',
            (r, g, b) if b > 150 && r < 100 && g < 100 => 'B',
            _ => '?',
        }
    })
    .collect()
}

/// A phone's film, kept a quarter turn from upright: every frame comes
/// upright, 160 by 320, its stored top-left (blue) at the top right, while
/// paused, sought, its sound track changed (the pipeline taken back to
/// READY) and playing.
#[test]
fn a_phone_s_film_is_handed_over_upright() {
    let dir = tempfile::tempdir().unwrap();
    let film = turned(&stored(dir.path()), -90, false);
    let mut worker = Worker::open(&film);
    let upright = ((160, 320), "RBRG".to_owned());
    assert_eq!(worker.frame(), upright, "its first frame");
    worker.control(Control::Seek(1.));
    assert_eq!(worker.frame(), upright, "after a seek");
    worker.control(Control::Audio(0));
    assert_eq!(worker.frame(), upright, "after a track change");
    worker.control(Control::Play(true));
    assert_eq!(worker.frame(), upright, "playing");
}

/// Each of the eight ways a display matrix shows a film, as ffmpeg shows
/// it: the size turned with a quarter turn, the corners where it puts them.
#[test]
fn every_way_a_film_is_shown_is_how_it_comes() {
    let dir = tempfile::tempdir().unwrap();
    let clip = stored(dir.path());
    for (degrees, mirrored, size, seen) in [
        (0, false, (320, 160), "BGRR"),
        (-90, false, (160, 320), "RBRG"),
        (180, false, (320, 160), "RRGB"),
        (90, false, (160, 320), "GRBR"),
        (0, true, (320, 160), "GBRR"),
        (-90, true, (160, 320), "BRGR"),
        (180, true, (320, 160), "RRBG"),
        (90, true, (160, 320), "RGRB"),
    ] {
        let film = turned(&clip, degrees, mirrored);
        let shown = Worker::open(&film).frame();
        assert_eq!(
            shown,
            (size, seen.to_owned()),
            "turned {degrees}, mirrored: {mirrored}"
        );
    }
}
