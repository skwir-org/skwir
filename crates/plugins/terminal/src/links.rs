//! Links in the terminal's text (ADR-029): those programs write (OSC 8),
//! and the addresses and paths found in what they print.
//!
//! A path is read from the folder the shell was in when its line was
//! printed, not when it is followed: following one takes the pane, and the
//! shell after it, elsewhere, and the next path of the same compiler
//! message must still lead where it did. So each address and path found is
//! stamped into its cells as a link of the engine's own
//! (`Cell::set_hyperlink`) the first time a screen shows it, with the folder
//! of that moment. The grid keeps a stamp through scrolling, the history
//! and reflow, and loses it with its text when a program writes over it.
//! Whether a path names a file is asked on a thread of its own (a disk may
//! be asleep): until it answers, and when nothing is there, the word stays
//! text. Lines that scroll past between two screens are never shown, so
//! never stamped then: shown later, they are read from the folder of that
//! later moment.
//!
//! Paths are found outside full-screen programs only: an editor's or a
//! pager's screen is its own, not output to act on. Addresses are found
//! everywhere.
//!
//! A stamp costs one allocation, shared by its cells, as a program's own
//! link does; a word found to name nothing wears one marker shared by all
//! such words (`NOWHERE`), so the history of a long log costs no more than
//! its links.

use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, CellExtra, Flags, Hyperlink};
use alacritty_terminal::term::{Term, TermMode};
use parking_lot::{Mutex, MutexGuard};
use skwir_core::terminal::Target;
use skwir_platform::links::{self, PathWord};

/// What the id of a stamp starts with. OSC 8 separates its parameters with
/// `:`, so no program can write an id that starts so.
const STAMP: &str = "skwir:";
/// Stamps remembered: the oldest go first, and their cells are text again.
const KEPT: usize = 4096;
/// Paths waiting for the filesystem's answer, at most: in a flood of
/// output, what does not fit waits to be stamped by a later screen.
const ASKED: usize = 1024;
/// Rows above the screen read to find the start of a line wrapped into it.
const WRAPPED: usize = 16;
/// The id of the marker on words asked about that name nothing: stamps
/// are numbered from 1.
const NOWHERE: &str = "skwir:0";

/// What a stamp leads to.
#[derive(Clone, Debug)]
enum Leads {
    /// An address: what it says.
    Address(String),
    /// A path the filesystem has not answered for yet.
    Asked,
    Path {
        path: PathBuf,
        line: Option<u32>,
        column: Option<u32>,
    },
    /// A word that names no file.
    Nowhere,
}

struct Stamp {
    /// The text it was stamped on, to tell it from text written since.
    text: String,
    leads: Leads,
}

/// The stamps on the grid, by number.
#[derive(Default)]
pub struct Stamps {
    next: u64,
    entries: HashMap<u64, Stamp>,
    order: VecDeque<u64>,
}

impl Stamps {
    fn add(&mut self, text: String, leads: Leads) -> u64 {
        self.next += 1;
        let id = self.next;
        self.entries.insert(id, Stamp { text, leads });
        self.order.push_back(id);
        while self.order.len() > KEPT {
            if let Some(old) = self.order.pop_front() {
                self.entries.remove(&old);
            }
        }
        id
    }

    fn of(&self, link: &Hyperlink) -> Option<&Stamp> {
        let id = link.id().strip_prefix(STAMP)?.parse().ok()?;
        self.entries.get(&id)
    }

    /// Where a cell's link leads, and whether a program wrote it: one a
    /// program wrote as it wrote it (a file of this machine is a path), a
    /// stamp once it leads somewhere.
    pub fn target(&self, link: &Hyperlink) -> Option<(Target, bool)> {
        if !link.id().starts_with(STAMP) {
            let uri = link.uri();
            let target = match skwir_platform::uri::path_of(uri) {
                Some(path) => Target::Path {
                    path,
                    line: None,
                    column: None,
                },
                None => Target::Uri(uri.to_owned()),
            };
            return Some((target, true));
        }
        match &self.of(link)?.leads {
            Leads::Address(address) => Some((Target::Uri(address.clone()), false)),
            Leads::Path { path, line, column } => Some((
                Target::Path {
                    path: path.clone(),
                    line: *line,
                    column: *column,
                },
                false,
            )),
            Leads::Asked | Leads::Nowhere => None,
        }
    }

    /// Whether any path still waits for the filesystem.
    #[cfg(test)]
    fn asking(&self) -> bool {
        self.entries
            .values()
            .any(|stamp| matches!(stamp.leads, Leads::Asked))
    }
}

/// A path to look for, from where its line was printed.
struct Ask {
    id: u64,
    word: PathWord,
    base: PathBuf,
}

/// What was found on one line, before it is stamped.
struct Found {
    cells: Vec<Point>,
    text: String,
    path: Option<PathWord>,
}

/// The links a session finds, and the thread that asks the filesystem.
pub struct Finder {
    stamps: Arc<Mutex<Stamps>>,
    asks: Option<SyncSender<Ask>>,
    /// What the cells of a word that names nothing share.
    nowhere: Arc<CellExtra>,
}

/// What a cell holding only `link` holds beside its character, made once
/// and shared by every cell of the link.
fn extra(link: Hyperlink) -> Arc<CellExtra> {
    let mut cell = Cell::default();
    cell.set_hyperlink(Some(link));
    cell.extra.unwrap_or_default()
}

/// Give `cells` the link `shared` holds: shared where a cell holds nothing
/// else, set on its own where it holds more (a combining mark, an
/// underline's colour).
fn mark(grid: &mut Grid<Cell>, cells: &[Point], shared: &Arc<CellExtra>, link: &Hyperlink) {
    for &point in cells {
        let cell = &mut grid[point];
        if cell.zerowidth().is_none_or(<[char]>::is_empty) && cell.underline_color().is_none() {
            cell.extra = Some(Arc::clone(shared));
        } else {
            cell.set_hyperlink(Some(link.clone()));
        }
    }
}

impl Finder {
    /// `wake` is called when a path was found to lead somewhere: the
    /// screen has a link more to publish.
    pub fn new(wake: impl Fn() + Send + 'static) -> Self {
        let stamps = Arc::new(Mutex::new(Stamps::default()));
        let (asks, asked) = std::sync::mpsc::sync_channel(ASKED);
        let answers = Arc::clone(&stamps);
        let started = std::thread::Builder::new()
            .name("skwir-terminal-links".into())
            .spawn(move || answer(&asked, &answers, &wake));
        if let Err(error) = &started {
            tracing::warn!(%error, "terminal: no thread to find paths on");
        }
        Self {
            stamps,
            asks: started.is_ok().then_some(asks),
            nowhere: extra(Hyperlink::new(Some(NOWHERE), String::new())),
        }
    }

    pub fn stamps(&self) -> MutexGuard<'_, Stamps> {
        self.stamps.lock()
    }

    /// Stamp the addresses and paths the screen shows that are not yet,
    /// paths read from `base`, the folder the shell is in now.
    pub fn stamp<T: EventListener>(&self, term: &mut Term<T>, base: Option<&Path>) {
        let paths = base.filter(|_| !term.mode().contains(TermMode::ALT_SCREEN));
        let found = find(term.grid(), paths.is_some());
        if found.is_empty() {
            return;
        }
        let mut stamps = self.stamps.lock();
        for Found { cells, text, path } in found {
            let grid = term.grid();
            let (mut ours, mut whole, mut theirs) = (None::<Hyperlink>, true, false);
            for &point in &cells {
                match grid[point].hyperlink() {
                    Some(link) if !link.id().starts_with(STAMP) => theirs = true,
                    Some(link) => match &ours {
                        Some(stamp) if *stamp != link => whole = false,
                        Some(_) => {}
                        None => ours = Some(link),
                    },
                    None => whole = false,
                }
            }
            // A program's own link is left as it wrote it, and a word asked
            // about already is not asked again.
            if theirs || whole && ours.as_ref().is_some_and(|link| link.id() == NOWHERE) {
                continue;
            }
            let stamped = ours.as_ref().and_then(|link| stamps.of(link));
            if whole && stamped.is_some_and(|stamp| stamp.text == text) {
                // Found to name nothing: the word wears the shared marker,
                // and its stamp is forgotten.
                if let Some(link) = ours
                    .filter(|_| stamped.is_some_and(|stamp| matches!(stamp.leads, Leads::Nowhere)))
                {
                    if let Some(id) = link.id().strip_prefix(STAMP).and_then(|id| id.parse().ok()) {
                        stamps.entries.remove(&id);
                    }
                    let marker = Hyperlink::new(Some(NOWHERE), String::new());
                    mark(term.grid_mut(), &cells, &self.nowhere, &marker);
                }
                continue;
            }
            let id = match path {
                None => match skwir_platform::uri::path_of(&text) {
                    // An address of a file is the file.
                    Some(file) => stamps.add(
                        text.clone(),
                        Leads::Path {
                            path: file,
                            line: None,
                            column: None,
                        },
                    ),
                    None if text
                        .get(..7)
                        .is_some_and(|s| s.eq_ignore_ascii_case("file://")) =>
                    {
                        continue;
                    }
                    None => stamps.add(text.clone(), Leads::Address(text.clone())),
                },
                Some(word) => {
                    let (Some(asks), Some(base)) = (&self.asks, paths) else {
                        continue;
                    };
                    let id = stamps.add(text.clone(), Leads::Asked);
                    let ask = Ask {
                        id,
                        word,
                        base: base.to_owned(),
                    };
                    match asks.try_send(ask) {
                        Ok(()) => id,
                        // Too much at once, or the thread is gone: a later
                        // screen stamps what is still shown.
                        Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                            stamps.entries.remove(&id);
                            continue;
                        }
                    }
                }
            };
            let link = Hyperlink::new(Some(format!("{STAMP}{id}")), text);
            mark(term.grid_mut(), &cells, &extra(link.clone()), &link);
        }
    }
}

/// The thread that asks the filesystem where each path leads.
fn answer(asked: &Receiver<Ask>, stamps: &Mutex<Stamps>, wake: &dyn Fn()) {
    for Ask { id, word, base } in asked {
        let path = links::resolve(&word.path, &base);
        let leads = path.is_some();
        if let Some(stamp) = stamps.lock().entries.get_mut(&id) {
            stamp.leads = match path {
                Some(path) => Leads::Path {
                    path,
                    line: word.line,
                    column: word.column,
                },
                None => Leads::Nowhere,
            };
        }
        if leads {
            wake();
        }
    }
}

/// Whether `line` goes on in the next row, the terminal having wrapped it.
fn wraps(grid: &Grid<Cell>, line: Line) -> bool {
    let last = Column(grid.columns().saturating_sub(1));
    grid[line][last].flags.contains(Flags::WRAPLINE)
}

/// The addresses, and paths when asked, on the lines the screen shows,
/// each line whole however the terminal wrapped it.
fn find(grid: &Grid<Cell>, paths: bool) -> Vec<Found> {
    let offset = i32::try_from(grid.display_offset()).unwrap_or(i32::MAX);
    let rows = i32::try_from(grid.screen_lines()).unwrap_or(i32::MAX);
    let bottom = Line(rows - 1 - offset);
    let topmost = grid.topmost_line();
    // A line that started above the screen is read from its start.
    let mut line = Line(-offset);
    for _ in 0..WRAPPED {
        let above = Line(line.0 - 1);
        if above < topmost || !wraps(grid, above) {
            break;
        }
        line = above;
    }
    let mut found = Vec::new();
    while line <= bottom {
        let mut text = String::new();
        let mut cells: Vec<(usize, Point)> = Vec::new();
        loop {
            for col in 0..grid.columns() {
                let cell = &grid[line][Column(col)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                cells.push((text.len(), Point::new(line, Column(col))));
                text.push(cell.c);
                if let Some(marks) = cell.zerowidth() {
                    text.extend(marks);
                }
            }
            let wrapped = wraps(grid, line);
            line = Line(line.0 + 1);
            if !wrapped || line > bottom {
                break;
            }
        }
        let addresses = links::urls(&text);
        let words = if paths {
            links::paths(&text)
                .into_iter()
                .filter(|word| !addresses.iter().any(|a| overlap(a, &word.range)))
                .collect()
        } else {
            Vec::new()
        };
        let mut take = |range: Range<usize>, path: Option<PathWord>| {
            let first = cells.partition_point(|(at, _)| *at < range.start);
            let last = cells.partition_point(|(at, _)| *at < range.end);
            found.push(Found {
                cells: cells[first..last].iter().map(|(_, point)| *point).collect(),
                text: text[range].to_owned(),
                path,
            });
        };
        for range in addresses {
            take(range, None);
        }
        for word in words {
            take(word.range.clone(), Some(word));
        }
    }
    found
}

fn overlap(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;
    use skwir_core::terminal::Link;

    use super::*;
    use crate::screen;

    struct Shell {
        term: Term<VoidListener>,
        parser: Processor,
        finder: Finder,
    }

    impl Shell {
        fn new(cols: usize, rows: usize) -> Self {
            Self {
                term: Term::new(Config::default(), &TermSize::new(cols, rows), VoidListener),
                parser: Processor::new(),
                finder: Finder::new(|| {}),
            }
        }

        fn print(&mut self, bytes: &[u8]) {
            self.parser.advance(&mut self.term, bytes);
        }

        /// The links of a screen published with the shell in `base`, once
        /// the filesystem has answered for every path.
        fn links(&mut self, base: Option<&Path>) -> Vec<Link> {
            self.finder.stamp(&mut self.term, base);
            let deadline = Instant::now() + Duration::from_secs(5);
            while self.finder.stamps().asking() {
                assert!(Instant::now() < deadline, "the paths were never looked for");
                std::thread::sleep(Duration::from_millis(5));
            }
            screen::build(&self.term, &self.finder.stamps(), screen::Selected::Read).links
        }
    }

    // Unix's paths.
    #[cfg(unix)]
    #[test]
    fn a_link_a_program_writes_is_kept_as_written() {
        let mut shell = Shell::new(40, 3);
        shell.print(b"\x1b]8;;file:///etc/hosts\x1b\\hosts\x1b]8;;\x1b\\ and \x1b]8;id=x;https://example.org/\x1b\\docs\x1b]8;;\x1b\\");
        let links = shell.links(None);
        assert_eq!(links.len(), 2);
        assert_eq!(
            links[0].target,
            Target::Path {
                path: "/etc/hosts".into(),
                line: None,
                column: None
            }
        );
        assert_eq!(links[0].spans, [(0, 0, 4)]);
        assert!(links[0].written);
        assert_eq!(links[1].target, Target::Uri("https://example.org/".into()));
        assert_eq!(links[1].spans, [(0, 10, 13)]);
    }

    #[test]
    fn an_address_wrapped_over_rows_is_one_link() {
        let mut shell = Shell::new(20, 4);
        shell.print(b"see https://example.org/a/long/path, ok");
        let links = shell.links(None);
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].target,
            Target::Uri("https://example.org/a/long/path".into())
        );
        assert_eq!(links[0].spans, [(0, 4, 19), (1, 0, 14)]);
        assert!(!links[0].written);
    }

    #[test]
    fn a_path_is_read_from_where_it_was_printed() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("src/x.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let mut shell = Shell::new(40, 4);
        shell.print(b"error: nothing here\r\n  --> src/x.rs:12:5\r\n");
        let links = shell.links(Some(root.path()));
        assert_eq!(links.len(), 1, "{links:?}");
        let at = Target::Path {
            path: file.clone(),
            line: Some(12),
            column: Some(5),
        };
        assert_eq!(links[0].target, at);
        assert_eq!(links[0].spans, [(1, 6, 18)]);
        // The shell went elsewhere since: the line still leads where it did.
        shell.print(b"$ ");
        let links = shell.links(Some(elsewhere.path()));
        assert_eq!(links.len(), 1, "{links:?}");
        assert_eq!(links[0].target, at);
    }

    #[test]
    fn text_written_over_a_stamp_is_read_again() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("abc"), "").unwrap();
        let mut shell = Shell::new(20, 2);
        shell.print(b"abd");
        assert!(shell.links(Some(root.path())).is_empty());
        // The last letter typed again, as a shell redraws its line.
        shell.print(b"\x08c");
        let links = shell.links(Some(root.path()));
        assert_eq!(links.len(), 1, "{links:?}");
        assert_eq!(links[0].spans, [(0, 0, 2)]);
    }

    #[test]
    fn a_word_that_names_nothing_wears_the_shared_marker() {
        let root = tempfile::tempdir().unwrap();
        let mut shell = Shell::new(40, 2);
        shell.print(b"nothing here");
        assert!(shell.links(Some(root.path())).is_empty());
        // The next screen puts the marker on, and forgets the stamps.
        assert!(shell.links(Some(root.path())).is_empty());
        let grid = shell.term.grid();
        let marked = |col: usize| {
            grid[Point::new(Line(0), Column(col))]
                .hyperlink()
                .map(|link| link.id().to_owned())
        };
        assert_eq!(marked(0).as_deref(), Some(NOWHERE));
        assert_eq!(marked(8).as_deref(), Some(NOWHERE));
        assert_eq!(marked(7), None, "a space is no word");
        assert!(shell.finder.stamps().entries.is_empty());
        // One allocation for every cell of every such word.
        let first = grid[Point::new(Line(0), Column(0))].extra.clone().unwrap();
        let other = grid[Point::new(Line(0), Column(9))].extra.clone().unwrap();
        assert!(Arc::ptr_eq(&first, &other));
    }

    #[test]
    fn a_full_screen_program_has_addresses_but_no_paths() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("notes.txt"), "").unwrap();
        let mut shell = Shell::new(40, 3);
        shell.print(b"\x1b[?1049hnotes.txt https://example.org");
        let links = shell.links(Some(root.path()));
        assert_eq!(links.len(), 1, "{links:?}");
        assert_eq!(links[0].target, Target::Uri("https://example.org".into()));
    }
}
