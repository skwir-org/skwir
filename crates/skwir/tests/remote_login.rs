//! Connect at login (ADR-087) as the service manager runs it, with no word
//! to the user's: units checked by `systemd-analyze verify` in folders of
//! the test's own, which asks no manager; their commands read back from the
//! unit as systemd reads it and run as it runs them, against servers of the
//! test's own; installing asked of a stand-in for `systemctl` that notes
//! what it is asked and with what environment. Never the user's systemd
//! manager, rclone configuration, ssh or agent: each test names its own.
//!
//! No manager of the test's own runs a unit here: systemd's user manager
//! moves every process of the cgroup it starts in into its `init.scope`,
//! and a cgroup of its own is one the user's manager delegates
//! (`systemd-run --user --scope -p Delegate=yes`), which is asking it.
//!
//! Skipped, saying why, where systemd-analyze, rclone, FUSE or OpenSSH's
//! server is missing.
#![cfg(target_os = "linux")]

use std::ffi::OsString;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use skwir_core::cancel::CancelToken;
use skwir_core::fs::reach;
use skwir_plugin_remote::address::Address;
use skwir_plugin_remote::connector::{self, Connector};
use skwir_plugin_remote::login::{self, AtLogin, Manager};
use skwir_plugin_remote::rclone::Rclone;
use skwir_plugin_remote::sshfs::Sshfs;

/// A folder of the test's on the home's file system, never `/tmp`.
fn fixture(name: &str) -> PathBuf {
    let base = dirs::home_dir()
        .unwrap()
        .join(".cache/skwir-tests/remote-login")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    base
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn find(program: &str) -> Option<PathBuf> {
    skwir_core::os_path::find_program(program)
}

/// Words that read differently in every grammar a unit passes through.
const HOSTILE: &[&str] = &[
    "plain",
    "two words",
    "it's",
    "say \"hi\"",
    "back\\slash",
    "trailing\\",
    "100%",
    "%%",
    "%h",
    "$HOME",
    "${HOME}",
    "$$",
    ";",
    "\\;",
    "tab\there",
    "new\nline",
    "é ü 中 🦀",
    "-dash",
    "\\x41",
];

/// The words systemd reads from a command line in a unit: quoted (double
/// or single quotes, C escapes), then `%%` as `%`, then, in an argument,
/// `$$` as `$` (systemd.syntax, systemd.service). `None` for what it would
/// read otherwise (a specifier, a variable, a lone `;`). Its prefix (`-`:
/// a failure ignored) is said apart.
fn systemd_words(line: &str) -> Option<(bool, Vec<String>)> {
    let ignored = line.starts_with('-');
    let line = line.trim_start_matches(['-', '@', ':', '+', '!']);
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(&first) = chars.peek() else { break };
        if first == ';' {
            return None;
        }
        let mut word = String::new();
        let mut quote = None;
        while let Some(c) = chars.next() {
            match (c, quote) {
                ('"' | '\'', None) => quote = Some(c),
                (c, Some(q)) if c == q => quote = None,
                (c, None) if c.is_whitespace() => break,
                ('\\', _) => match chars.next()? {
                    'n' => word.push('\n'),
                    't' => word.push('\t'),
                    'r' => word.push('\r'),
                    's' => word.push(' '),
                    'x' => {
                        let hex: String = [chars.next()?, chars.next()?].iter().collect();
                        word.push(char::from(u8::from_str_radix(&hex, 16).ok()?));
                    }
                    c @ ('\\' | '"' | '\'') => word.push(c),
                    _ => return None,
                },
                (c, _) => word.push(c),
            }
        }
        words.push(word);
    }
    let mut read = Vec::with_capacity(words.len());
    for (at, word) in words.into_iter().enumerate() {
        let mut out = String::new();
        let mut chars = word.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '%' if chars.next_if_eq(&'%').is_some() => out.push('%'),
                '%' => return None,
                '$' if at > 0 && chars.next_if_eq(&'$').is_some() => out.push('$'),
                '$' if at > 0 => return None,
                c => out.push(c),
            }
        }
        read.push(out);
    }
    Some((ignored, read))
}

/// The command lines of `unit` whose key is `key`, read as systemd reads
/// them.
fn commands(unit: &str, key: &str) -> Vec<(bool, Vec<String>)> {
    unit.lines()
        .filter_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .map(|line| systemd_words(line).unwrap_or_else(|| panic!("systemd reads {line} otherwise")))
        .collect()
}

/// Run a unit's command lines of `key` as systemd runs them, one after
/// another, the first failing one not ignored ending them.
fn run_commands(unit: &str, key: &str) {
    for (ignored, words) in commands(unit, key) {
        let status = Command::new(&words[0])
            .args(&words[1..])
            .stdin(Stdio::null())
            .status()
            .unwrap();
        assert!(ignored || status.success(), "{key} {words:?}: {status}");
    }
}

/// `unit`, written as `name` in a folder of units of `base`'s own, as
/// `systemd-analyze verify` judges it: with only that folder, a runtime
/// folder and a home of its own, it asks no manager (traced: it connects
/// nowhere, and its temporary folders are gone after). `None`, saying why,
/// where it is missing. `base` holds no `:`.
fn verified(base: &Path, name: &str, unit: &str) -> Option<Result<(), String>> {
    let Some(analyze) = find("systemd-analyze") else {
        eprintln!("not verified: systemd-analyze is not installed");
        return None;
    };
    let units = base.join("verify/config/systemd/user");
    for folder in [
        &units,
        &base.join("verify/runtime"),
        &base.join("verify/home"),
    ] {
        std::fs::create_dir_all(folder).unwrap();
    }
    // Its argument's first `:` starts the name the file is read as: the
    // file's own name holds none, the unit's may (`rclone:--gdrive-`).
    let file = units.join("checked.service");
    std::fs::write(&file, unit).unwrap();
    let mut named = file.clone().into_os_string();
    named.push(format!(":{name}"));
    let out = Command::new(analyze)
        .env_clear()
        .env("HOME", base.join("verify/home"))
        .env("XDG_RUNTIME_DIR", base.join("verify/runtime"))
        .env("XDG_CONFIG_HOME", base.join("verify/config"))
        .args(["--user", "verify", "--man=no"])
        .arg(named)
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr).into_owned();
    Some(if out.status.success() && said.trim().is_empty() {
        Ok(())
    } else {
        Err(said)
    })
}

/// A program at `path` that writes its arguments to `out`, each ended by a
/// NUL.
fn recorder(path: &Path, out: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        format!("#!/bin/sh\nprintf '%s\\0' \"$@\" > '{}'\n", out.display()),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn recorded(out: &Path) -> Vec<String> {
    let bytes = std::fs::read(out).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    text.strip_suffix('\0')
        .map(|t| t.split('\0').map(str::to_owned).collect())
        .unwrap_or_default()
}

/// A unit's name is systemd's own escaping of its path, and every word of
/// its commands is read back by systemd as written: its program found at a
/// path holding what systemd's grammars take apart, its arguments reaching
/// the program as they were.
#[test]
fn a_login_unit_is_read_by_systemd_as_written() {
    let base = fixture("verify");
    if let Some(escape) = find("systemd-escape") {
        for id in [
            "rclone/rclone://gdrive/",
            "sshfs/sftp://me@example.com:2222/",
            "x/.a b-c\\d%e$f'g\"h é",
        ] {
            let out = Command::new(&escape)
                .arg("--")
                .arg(format!("skwir/remote/{id}"))
                .output()
                .unwrap();
            assert!(out.status.success());
            assert_eq!(
                format!("{}.service", String::from_utf8_lossy(&out.stdout).trim()),
                login::unit_name(id),
                "{id}"
            );
        }
    } else {
        eprintln!("names not compared: systemd-escape is not installed");
    }
    // A path systemd takes as it is (it refuses one holding a quote or a
    // backslash: the shell runs that one, as the next shows).
    for (folder, through_shell) in [("tools 100% $x é 中;", false), ("o'brien \"q\" \\b", true)] {
        let program = base.join(folder).join("record args");
        let out = base.join(format!("argv-{through_shell}"));
        recorder(&program, &out);
        let at = AtLogin {
            id: "sshfs/sftp://me@example.com:2222/".to_owned(),
            title: "me@example.com:2222 at 100%".to_owned(),
            program: program.clone(),
            args: HOSTILE.iter().map(OsString::from).collect(),
            point: base.join("point 'q' \"d\" $x %y \\z é"),
            folders: vec![base.join("sockets 'q'")],
            leftovers: vec![base.join("sockets 'q'/a $x.sock")],
            notifies: false,
        };
        let unit = login::systemd_unit(&at);
        if let Some(verdict) = verified(&base, &login::unit_name(&at.id), &unit) {
            verdict.unwrap_or_else(|said| panic!("systemd refused it: {said}\n{unit}"));
        }
        let start = commands(&unit, "ExecStart");
        assert_eq!(start.len(), 1);
        let words = &start[0].1;
        if through_shell {
            assert_eq!(words[0], "/bin/sh");
            assert_eq!(words[3], program.to_str().unwrap());
        } else {
            assert_eq!(words[0], program.to_str().unwrap());
        }
        let status = Command::new(&words[0]).args(&words[1..]).status().unwrap();
        assert!(status.success());
        assert_eq!(recorded(&out), HOSTILE, "what the program was given");
        // What it starts with makes its folders, whatever their names.
        run_commands(&unit, "ExecStartPre");
        assert!(at.point.is_dir() && at.folders[0].is_dir());
        run_commands(&unit, "ExecStopPost");
        assert!(!at.point.exists(), "its point removed once it ended");
        std::fs::remove_dir(&at.folders[0]).unwrap();
    }
    std::fs::remove_dir_all(&base).unwrap();
}

/// `rclone serve <kind>` of `root` on a port of its own, stopped when
/// dropped.
struct Served {
    child: Child,
    port: u16,
}

impl Served {
    fn start(kind: &str, root: &Path) -> Served {
        let port = free_port();
        let child = Command::new("rclone")
            .args(["serve", kind])
            .arg(root)
            .args(["--addr", &format!("127.0.0.1:{port}")])
            .arg("--config")
            .arg(root.with_extension("conf"))
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(root.with_extension(format!("{kind}.log"))).unwrap())
            .spawn()
            .expect("start rclone serve");
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "the server never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
        Served { child, port }
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A unit's mount command, started as systemd starts it: its environment
/// told where to say it is ready (`NOTIFY_SOCKET`). Killed when dropped.
struct Started {
    child: Child,
    notify: Option<UnixDatagram>,
}

impl Started {
    fn start(unit: &str, notify: Option<&Path>) -> Started {
        let start = commands(unit, "ExecStart");
        assert_eq!(start.len(), 1, "{unit}");
        let words = &start[0].1;
        let mut command = Command::new(&words[0]);
        command.args(&words[1..]).stdin(Stdio::null());
        let notify = notify.map(|path| {
            let _ = std::fs::remove_file(path);
            let socket = UnixDatagram::bind(path).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_millis(200)))
                .unwrap();
            command.env("NOTIFY_SOCKET", path);
            socket
        });
        Started {
            child: command.spawn().unwrap(),
            notify,
        }
    }

    /// Until it says it is ready (`READY=1`), as `Type=notify` waits.
    fn ready(&mut self) {
        let socket = self.notify.as_ref().unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut buffer = [0; 4096];
        loop {
            if let Ok(n) = socket.recv(&mut buffer)
                && String::from_utf8_lossy(&buffer[..n])
                    .lines()
                    .any(|line| line == "READY=1")
            {
                return;
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                panic!("it ended before it was ready: {status}");
            }
            assert!(Instant::now() < deadline, "it never said it was ready");
        }
    }

    /// Until it ends, as its manager waits once it is stopped.
    fn ended(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "it never ended");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The mount at `point` now, its program's or not.
fn mounted_at(point: &Path) -> Option<std::sync::Arc<reach::Remote>> {
    reach::changed();
    reach::of(point).filter(|remote| remote.point == point)
}

/// Until `point` is mounted, within `seconds`.
fn until_mounted(point: &Path, seconds: u64) -> std::sync::Arc<reach::Remote> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        if let Some(remote) = mounted_at(point) {
            return remote;
        }
        assert!(
            Instant::now() < deadline,
            "never mounted at {}",
            point.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Whether the mount table names a mount under `base` or at `point`: none
/// is left behind.
fn left_mounted(base: &Path, point: &Path) -> bool {
    let table = std::fs::read_to_string("/proc/self/mountinfo").unwrap();
    // Its paths with their spaces as `\040`, as the table writes them.
    let written = |path: &Path| path.to_string_lossy().replace(' ', "\\040");
    table.contains(&written(base)) || table.contains(&written(point))
}

/// A stand-in for `systemctl` at `base/systemctl`, noting each call's
/// words in `base/systemctl.log` and its environment in
/// `base/systemctl.env`; `enable` and `disable` link the unit as systemd
/// does, in `units`; `start` fails where `base/fail-start` is. Its
/// environment holds no `PATH`: it names its programs whole.
fn fake_systemctl(base: &Path, units: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let program = base.join("systemctl");
    let wants = units.join("default.target.wants");
    let named = |name: &str| find(name).unwrap().display().to_string();
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$*\" >> '{log}'\n\
             '{env_program}' > '{env}'\n\
             for name; do :; done\n\
             case \"$2\" in\n\
             enable) '{mkdir}' -p '{wants}' && '{ln}' -sf \"../$name\" '{wants}'/\"$name\" ;;\n\
             disable) '{rm}' -f '{wants}'/\"$name\" ;;\n\
             start) [ -e '{fail}' ] && {{ echo \"Job for $name failed.\" >&2; exit 1; }} ;;\n\
             esac\n\
             exit 0\n",
            log = base.join("systemctl.log").display(),
            env = base.join("systemctl.env").display(),
            wants = wants.display(),
            fail = base.join("fail-start").display(),
            env_program = named("env"),
            mkdir = named("mkdir"),
            ln = named("ln"),
            rm = named("rm"),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    program
}

/// What the stand-in was asked since asked last.
fn asked(base: &Path) -> Vec<String> {
    let log = base.join("systemctl.log");
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    text.lines().map(str::to_owned).collect()
}

/// The manager a test's stand-in is, its units in `base/units`.
fn fake_manager(base: &Path) -> Manager {
    let units = base.join("units");
    let runtime = base.join("runtime");
    std::fs::create_dir_all(&runtime).unwrap();
    Manager {
        program: fake_systemctl(base, &units),
        folder: units,
        runtime: Some(runtime),
    }
}

/// Installing writes the unit, asks its manager to load and enable it and
/// to start it now where nothing is mounted at its point, each call with
/// the test's runtime folder alone in its environment; removing it disables
/// and stops it; one whose start failed leaves nothing behind.
#[test]
fn installing_sets_the_unit_up_and_takes_it_back_where_it_fails() {
    let base = fixture("install");
    let manager = fake_manager(&base);
    let at = AtLogin {
        id: "rclone/rclone://gdrive/".to_owned(),
        title: "gdrive".to_owned(),
        program: PathBuf::from("/opt/rclone/rclone"),
        args: vec!["mount".into(), "gdrive:".into()],
        point: base.join("point"),
        folders: Vec::new(),
        leftovers: Vec::new(),
        notifies: true,
    };
    let name = login::unit_name(&at.id);
    let cancel = CancelToken::new();
    assert!(!manager.installed(&at.id));
    manager.install(&at, &cancel).unwrap();
    assert_eq!(
        asked(&base),
        [
            "--user daemon-reload".to_owned(),
            format!("--user enable --quiet {name}"),
            format!("--user start {name}"),
        ]
    );
    // Its manager is told of the runtime folder alone: never the user's.
    let env = std::fs::read_to_string(base.join("systemctl.env")).unwrap();
    assert!(
        env.lines()
            .any(|l| l == format!("XDG_RUNTIME_DIR={}", base.join("runtime").display()))
    );
    for variable in [
        "HOME=",
        "XDG_CONFIG_HOME=",
        "DBUS_SESSION_BUS_ADDRESS=",
        "RCLONE_CONFIG=",
    ] {
        assert!(!env.lines().any(|l| l.starts_with(variable)), "{env}");
    }
    assert_eq!(
        std::fs::read_to_string(manager.folder.join(&name)).unwrap(),
        login::systemd_unit(&at)
    );
    assert!(manager.installed(&at.id));
    manager.uninstall(&at.id, &at.point, &cancel).unwrap();
    assert_eq!(
        asked(&base),
        [
            format!("--user disable --quiet --now {name}"),
            "--user daemon-reload".to_owned(),
        ],
        "nothing mounted: a run failing again and again is stopped too"
    );
    assert!(!manager.folder.join(&name).exists());
    assert!(!manager.installed(&at.id));
    // A start that fails: taken back, and said with where to read why.
    std::fs::write(base.join("fail-start"), "").unwrap();
    let error = manager.install(&at, &cancel).unwrap_err().to_string();
    assert!(
        error.contains("failed") && error.contains(&format!("journalctl --user -u {name}")),
        "{error}"
    );
    assert!(!manager.folder.join(&name).exists());
    assert!(!manager.installed(&at.id));
    assert_eq!(
        asked(&base)[3..],
        [
            format!("--user disable --quiet --now {name}"),
            "--user daemon-reload".to_owned(),
        ]
    );
    std::fs::remove_dir_all(&base).unwrap();
}

/// Why rclone or FUSE cannot be tried here, where it cannot.
fn no_rclone() -> Option<&'static str> {
    if find("rclone").is_none() {
        return Some("rclone is not installed");
    }
    if find("fusermount3").is_none() && find("fusermount").is_none() {
        return Some("FUSE (fusermount3) is not installed");
    }
    None
}

/// A remote of rclone's configuration, as its unit mounts it at login: in
/// the foreground at Skwir's own point, rclone saying it is ready, Skwir
/// knowing the mount as its own, with its remote control; let go of as the
/// unit stops it, and again where its program died, nothing left.
#[test]
fn an_rclone_remote_mounts_at_login_as_its_unit_runs_it() {
    if let Some(why) = no_rclone() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("rclone");
    let served_root = base.join("served");
    std::fs::create_dir_all(&served_root).unwrap();
    std::fs::write(served_root.join("hello é.txt"), "from the server").unwrap();
    let served = Served::start("webdav", &served_root);
    let name = format!("login-{}", std::process::id());
    let config = base.join("rclone client.conf");
    std::fs::write(
        &config,
        format!(
            "[{name}]\ntype = webdav\nurl = http://127.0.0.1:{}\nvendor = other\n",
            served.port
        ),
    )
    .unwrap();
    let rclone = Rclone::with_config(config.clone());
    let address = Address::parse(&format!("rclone://{name}/Some/Folder")).unwrap();
    let top = address.top();
    let cancel = CancelToken::new();
    let at = AtLogin::rclone(&rclone, &address, &cancel).unwrap();
    // Where Skwir mounts it itself, its configuration named.
    assert_eq!(
        at.point,
        connector::mounts_folder()
            .unwrap()
            .join(connector::point_name("rclone", &top))
    );
    assert_eq!(at.id, login::id_of("rclone", &top));
    assert_eq!(login::point_of("rclone", &address).unwrap(), at.point);
    let given = at.args.iter().position(|a| a == "--config").unwrap();
    assert_eq!(at.args[given + 1], config.clone().into_os_string());
    assert!(at.notifies);
    let unit = login::systemd_unit(&at);
    if let Some(verdict) = verified(&base, &login::unit_name(&at.id), &unit) {
        verdict.unwrap_or_else(|said| panic!("systemd refused it: {said}\n{unit}"));
    }
    run_commands(&unit, "ExecStartPre");
    assert!(at.point.is_dir());
    let mut started = Started::start(&unit, Some(&base.join("notify.sock")));
    started.ready();
    let remote = mounted_at(&at.point).expect("mounted once it said it was ready");
    assert_eq!(remote.fstype, "fuse.rclone");
    let recognised = rclone.recognises(&remote).unwrap();
    assert!(recognised.ours, "Skwir's own mount");
    assert_eq!(recognised.address, Some(top.clone()));
    assert_eq!(
        std::fs::read_to_string(at.point.join("hello é.txt")).unwrap(),
        "from the server"
    );
    assert!(
        at.leftovers.iter().all(|socket| socket.exists()),
        "its remote control serves Skwir's copies"
    );
    // Turned on while mounted: set up for the next login, nothing started;
    // turned off: its mount kept.
    let manager = fake_manager(&base);
    manager.install(&at, &cancel).unwrap();
    let unit_name = login::unit_name(&at.id);
    assert_eq!(
        asked(&base),
        [
            "--user daemon-reload".to_owned(),
            format!("--user enable --quiet {unit_name}"),
        ]
    );
    manager.uninstall(&at.id, &at.point, &cancel).unwrap();
    assert_eq!(
        asked(&base),
        [
            format!("--user disable --quiet {unit_name}"),
            "--user daemon-reload".to_owned(),
        ]
    );
    // Stopped as its manager stops it.
    run_commands(&unit, "ExecStop");
    assert!(started.ended().success(), "rclone ends once let go of");
    run_commands(&unit, "ExecStopPost");
    assert!(mounted_at(&at.point).is_none());
    assert!(!at.point.exists(), "its point removed");
    assert!(at.leftovers.iter().all(|socket| !socket.exists()));
    // Its program killed: what it left is let go of once it ended.
    run_commands(&unit, "ExecStartPre");
    let mut started = Started::start(&unit, Some(&base.join("notify.sock")));
    started.ready();
    assert!(mounted_at(&at.point).is_some());
    let _ = started.child.kill();
    let _ = started.ended();
    let error = std::fs::symlink_metadata(&at.point).unwrap_err();
    assert_eq!(
        error.kind(),
        std::io::ErrorKind::NotConnected,
        "a mount whose program is gone"
    );
    run_commands(&unit, "ExecStopPost");
    assert!(mounted_at(&at.point).is_none(), "let go of");
    assert!(!at.point.exists());
    assert!(at.leftovers.iter().all(|socket| !socket.exists()));
    assert!(!left_mounted(&base, &at.point));
    drop(started);
    drop(served);
    std::fs::remove_dir_all(&base).unwrap();
}

/// What would need an answer at login is refused, saying why.
#[test]
fn what_needs_an_answer_at_login_is_refused() {
    if find("rclone").is_none() {
        eprintln!("skipped: rclone is not installed");
        return;
    }
    let base = fixture("refused");
    let cancel = CancelToken::new();
    let refusal = |rclone: &Rclone, address: &str| {
        AtLogin::rclone(rclone, &Address::parse(address).unwrap(), &cancel)
            .unwrap_err()
            .to_string()
    };
    let encrypted = base.join("encrypted.conf");
    std::fs::write(
        &encrypted,
        "# Encrypted rclone configuration File\n\nRCLONE_ENCRYPT_V0:\nAAAA\n",
    )
    .unwrap();
    let error = refusal(&Rclone::with_config(encrypted), "rclone://drive/");
    assert!(error.contains("encrypted"), "{error}");
    let plain = base.join("plain.conf");
    std::fs::write(&plain, "[other]\ntype = memory\n").unwrap();
    let error = refusal(&Rclone::with_config(plain.clone()), "rclone://drive/");
    assert!(error.contains("names no remote “drive”"), "{error}");
    let error = refusal(&Rclone::with_config(plain), "dav://127.0.0.1:8080/");
    assert!(error.contains("password"), "{error}");
    std::fs::remove_dir_all(&base).unwrap();
}

/// OpenSSH's server on a port of its own, its key the test's.
struct OwnSshd {
    child: Child,
    port: u16,
}

impl OwnSshd {
    /// `None`, saying why, where OpenSSH's server is missing.
    fn start(base: &Path) -> Option<OwnSshd> {
        let (Some(sshd), Some(keygen)) = (find("sshd"), find("ssh-keygen")) else {
            eprintln!("skipped: OpenSSH's server is not installed");
            return None;
        };
        let sftp = std::fs::canonicalize(&sshd)
            .ok()?
            .parent()?
            .parent()?
            .join("libexec/sftp-server");
        if !sftp.exists() {
            eprintln!("skipped: no sftp-server beside sshd");
            return None;
        }
        for key in ["key", "hostkey"] {
            let made = Command::new(&keygen)
                .args(["-q", "-t", "ed25519", "-N", "", "-C", "test", "-f"])
                .arg(base.join(key))
                .status()
                .unwrap();
            assert!(made.success());
        }
        std::fs::copy(base.join("key.pub"), base.join("authorized_keys")).unwrap();
        let port = free_port();
        let config = base.join("sshd_config");
        std::fs::write(
            &config,
            format!(
                "Port {port}\nListenAddress 127.0.0.1\nHostKey {}\nPidFile {}\n\
                 AuthorizedKeysFile {}\nUsePAM no\nStrictModes no\n\
                 PasswordAuthentication no\nKbdInteractiveAuthentication no\n\
                 Subsystem sftp {}\nLogLevel ERROR\n",
                base.join("hostkey").display(),
                base.join("sshd.pid").display(),
                base.join("authorized_keys").display(),
                sftp.display(),
            ),
        )
        .unwrap();
        let child = Command::new(sshd)
            .arg("-D")
            .arg("-f")
            .arg(&config)
            .arg("-E")
            .arg(base.join("sshd.log"))
            .spawn()
            .expect("start sshd");
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "sshd never listened");
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(OwnSshd { child, port })
    }
}

impl Drop for OwnSshd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// sshfs given what reaches the test's server alone: its key, known hosts
/// of the test's own, no agent, no configuration of the user's.
fn own_sshfs(base: &Path, key: &Path, strict: &str) -> Sshfs {
    Sshfs {
        ssh_options: vec![
            format!("IdentityFile={}", key.display()),
            "IdentitiesOnly=yes".to_owned(),
            "IdentityAgent=none".to_owned(),
            format!("UserKnownHostsFile={}", base.join("known_hosts").display()),
            format!("StrictHostKeyChecking={strict}"),
        ],
        ssh_config: Some(PathBuf::from("/dev/null")),
        ..Sshfs::default()
    }
}

/// An SFTP address whose server takes a key, as its unit mounts it at
/// login: in the foreground, asking nothing, at Skwir's own point; one
/// whose server would ask, or whose identity ssh does not know, refused.
#[test]
fn an_sftp_address_mounts_at_login_as_its_unit_runs_it() {
    if find("sshfs").is_none() {
        eprintln!("skipped: sshfs is not installed");
        return;
    }
    let base = fixture("sshfs");
    let Some(sshd) = OwnSshd::start(&base) else {
        return;
    };
    std::fs::write(base.join("hello.txt"), "over sftp").unwrap();
    let address = Address::parse(&format!("sftp://127.0.0.1:{}/", sshd.port)).unwrap();
    let cancel = CancelToken::new();
    // Not known yet, and nobody to say it is the one: refused.
    let unknown = own_sshfs(&base, &base.join("key"), "yes");
    let error = AtLogin::sshfs(&unknown, &address, &cancel)
        .unwrap_err()
        .to_string();
    assert!(error.contains("identity"), "{error}");
    // A key the server does not take: it would ask for a password.
    let sshfs = own_sshfs(&base, &base.join("key"), "accept-new");
    let keyless = own_sshfs(&base, &base.join("no-such-key"), "accept-new");
    let error = AtLogin::sshfs(&keyless, &address, &cancel)
        .unwrap_err()
        .to_string();
    assert!(error.contains("password"), "{error}");
    let at = AtLogin::sshfs(&sshfs, &address, &cancel).unwrap();
    assert_eq!(
        at.point,
        connector::mounts_folder()
            .unwrap()
            .join(connector::point_name("sshfs", &address.top()))
    );
    assert!(!at.notifies);
    assert!(
        at.args
            .windows(2)
            .any(|w| w[0] == "-o" && w[1] == "BatchMode=yes")
    );
    assert!(at.args.iter().any(|a| a == "-f"));
    let unit = login::systemd_unit(&at);
    if let Some(verdict) = verified(&base, &login::unit_name(&at.id), &unit) {
        verdict.unwrap_or_else(|said| panic!("systemd refused it: {said}\n{unit}"));
    }
    run_commands(&unit, "ExecStartPre");
    let mut started = Started::start(&unit, None);
    let remote = until_mounted(&at.point, 30);
    assert_eq!(remote.fstype, "fuse.sshfs");
    let recognised = sshfs.recognises(&remote).unwrap();
    assert!(recognised.ours, "Skwir's own mount");
    assert_eq!(recognised.address, Some(address.top()));
    let there = at
        .point
        .join(base.strip_prefix("/").unwrap())
        .join("hello.txt");
    assert_eq!(std::fs::read_to_string(there).unwrap(), "over sftp");
    run_commands(&unit, "ExecStop");
    assert!(started.ended().success(), "sshfs ends once let go of");
    run_commands(&unit, "ExecStopPost");
    assert!(mounted_at(&at.point).is_none());
    assert!(!at.point.exists());
    assert!(!left_mounted(&base, &at.point));
    drop(started);
    drop(sshd);
    std::fs::remove_dir_all(&base).unwrap();
}

/// Until `ready` holds, pumping `app`, for at most `seconds`.
fn until(app: &skwir_core::app::App, seconds: u64, what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while !ready() {
        app.pump();
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Connect at Login from a remote's menu (ADR-087): its unit set up with
/// the service manager (the test's stand-in), the item checked once it
/// is, and turned off from the same item.
#[test]
fn connect_at_login_is_turned_on_and_off_from_a_remote_s_menu() {
    use std::sync::Arc;
    if let Some(why) = no_rclone() {
        eprintln!("skipped: {why}");
        return;
    }
    let base = fixture("menu");
    std::fs::create_dir_all(base.join("disk")).unwrap();
    let config = base.join("rclone.conf");
    std::fs::write(
        &config,
        format!(
            "[disk]\ntype = alias\nremote = {}\n",
            base.join("disk").display()
        ),
    )
    .unwrap();
    let manager = fake_manager(&base);
    let app =
        skwir_core::app::App::new(skwir_core::app::AppConfig::ephemeral(base.join("app"))).unwrap();
    let rclone = Rclone::with_config(config);
    let remotes = skwir_plugin_remote::Remotes::with(vec![Arc::new(rclone.clone())])
        .with_rclone(rclone)
        .with_login(manager.clone());
    app.plugins.add(
        Arc::new(skwir_plugin_remote::RemotePlugin::with(remotes)),
        &app,
    );
    let item = |title: &str| {
        app.registry
            .places
            .find(|p| p.title.starts_with("disk"))
            .and_then(|p| p.menu.into_iter().find(|i| i.title == title))
    };
    until(&app, 30, "its menu offers to connect at login", || {
        item("Connect at Login").is_some()
    });
    app.dispatch(&item("Connect at Login").unwrap().command)
        .unwrap();
    let id = login::id_of("rclone", &Address::parse("rclone://disk/").unwrap());
    until(&app, 60, "its unit was set up", || manager.installed(&id));
    let name = login::unit_name(&id);
    let said = asked(&base);
    assert!(
        said.contains(&format!("--user enable --quiet {name}")),
        "{said:?}"
    );
    assert!(said.contains(&format!("--user start {name}")), "{said:?}");
    until(&app, 30, "its menu says it connects at login", || {
        item("Connect at Login ✓").is_some()
    });
    app.dispatch(&item("Connect at Login ✓").unwrap().command)
        .unwrap();
    until(&app, 60, "its unit was taken out", || {
        !manager.installed(&id)
    });
    assert!(!manager.folder.join(&name).exists());
    until(&app, 30, "its menu offers it again", || {
        item("Connect at Login").is_some()
    });
    std::fs::remove_dir_all(&base).unwrap();
}
