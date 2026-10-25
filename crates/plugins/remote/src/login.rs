//! Connect at login (ADR-087): a remote the user picks is mounted when they
//! log in, by the system's own service manager, never a daemon of Skwir's:
//! a systemd user unit on Linux, a launch agent on macOS, a Task Scheduler
//! logon task on Windows. It runs the mount command Skwir runs, in the
//! foreground, at the point Skwir mounts the address at
//! (`connector::mounts_folder`, `point_name`): Skwir knows the mount as its
//! own (`mounter::ours`), and Eject lets go of it as of any other. Nothing
//! is kept but the service manager's file.
//!
//! What would need an answer at login is refused: nobody is there to give
//! it. An SFTP address is logged in to once, as the unit's ssh will
//! (`BatchMode`), and refused where the server wants a password or a key's
//! passphrase, or ssh does not know its identity yet; rclone's remotes
//! where its configuration is encrypted.
//!
//! Before it runs, and once it ends, what a run that died left is let go
//! of, as Skwir lets go of it before mounting (`mounter::point_for`): a
//! mount whose program is gone, its sockets. Turned off, the entry is
//! removed and its manager forgets it; a mount it carries stays until Eject
//! or the end of the session, and one that keeps failing is stopped. Its
//! manager forgets the running entry's own commands with its file (systemd
//! reloads a unit whose file is gone as not found): such a mount is then
//! as one Skwir made. Eject lets go of it and its point; let go of
//! otherwise, what it leaves is let go of by the next mount there.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use skwir_core::cancel::CancelToken;

use crate::address::{Address, Scheme};
use crate::connector::{self, Connector as _};

/// How long its manager may take to start a mount now: a cloud's first
/// listing, as the connectors wait.
const PATIENCE: Duration = Duration::from_secs(120);

/// What runs at login: a remote's mount command as Skwir runs it, in the
/// foreground, its service manager waiting on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtLogin {
    /// Names it among the others ([`id_of`]: `rclone/rclone://gdrive/`).
    pub id: String,
    /// What the user calls the remote (`Address::label`).
    pub title: String,
    /// The program, found on `PATH` as it was made.
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Where it mounts: made before it runs, removed once it ends.
    pub point: PathBuf,
    /// Other folders made before it runs: its sockets'.
    pub folders: Vec<PathBuf>,
    /// What a run that died leaves (its sockets): removed before the next
    /// and after it ends, where its mount died with it.
    pub leftovers: Vec<PathBuf>,
    /// It tells its manager once mounted (`sd_notify`: rclone does).
    pub notifies: bool,
    /// The login its remote control serves behind, kept beside the mounts
    /// as long as the entry is (`winfsp`: on Windows, where rclone is
    /// given nothing in its environment).
    pub control: Option<crate::rc::Login>,
}

/// What names the entry of `connector` mounting `address` at login: the
/// connector and the address's top (`rclone/rclone://gdrive/`), one entry
/// per mount point.
pub fn id_of(connector: &str, address: &Address) -> String {
    format!("{connector}/{}", address.top())
}

/// Where `connector` mounts `address`'s top, as Skwir does
/// (`mounter::point_for`), at login too: not made here.
///
/// # Errors
/// Where the user has no runtime folder.
pub fn point_of(connector: &str, address: &Address) -> anyhow::Result<PathBuf> {
    let folder = connector::mounts_folder()
        .ok_or_else(|| anyhow::anyhow!("there is no runtime folder to mount in"))?;
    Ok(folder.join(connector::point_name(connector, &address.top())))
}

impl AtLogin {
    /// A remote of rclone's configuration (`rclone://gdrive/`), mounted as
    /// the rclone connector mounts it, in the foreground: rclone tells its
    /// manager once mounted. The configuration is named (`--config`): the
    /// one Skwir reads, whatever the manager's environment says. Reads the
    /// configuration: on a worker.
    ///
    /// # Errors
    /// For an address of another kind (Skwir gives a server's password,
    /// and is not there at login), a configuration that is encrypted or
    /// names no such remote, or no rclone.
    pub fn rclone(
        with: &crate::rclone::Rclone,
        address: &Address,
        cancel: &CancelToken,
    ) -> anyhow::Result<AtLogin> {
        let (program, config, top) = rclone_checked(with, address, cancel)?;
        #[cfg(unix)]
        {
            rclone_on_unix(with, program, config, &top)
        }
        #[cfg(windows)]
        {
            rclone_on_windows(with, program, config, &top, cancel)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (program, config, top);
            Err(skwir_core::system::not_yet("Mounting rclone's remotes").into())
        }
    }

    /// An SFTP address mounted by sshfs as the sshfs connector mounts it, in
    /// the foreground, its ssh asking nothing (`BatchMode`): a key ssh's
    /// agent holds, or one with no passphrase. Logs in to the server once
    /// to know it asks nothing: on a worker.
    ///
    /// # Errors
    /// Where the server wants a password or a passphrase, ssh does not know
    /// its identity yet, it was not reached, or sshfs is missing.
    pub fn sshfs(
        with: &crate::sshfs::Sshfs,
        address: &Address,
        cancel: &CancelToken,
    ) -> anyhow::Result<AtLogin> {
        let top = address.top();
        anyhow::ensure!(
            top.scheme == Scheme::Sftp,
            "sshfs connects SFTP addresses alone"
        );
        top.ssh_user()?;
        if let Some(why) = with.unavailable() {
            anyhow::bail!("{why}");
        }
        let program = skwir_core::os_path::find_program("sshfs")
            .ok_or_else(|| anyhow::anyhow!("sshfs is not installed"))?;
        logs_in_alone(with, &top, cancel)?;
        let point = point_of(with.id(), &top)?;
        let control = crate::ssh_mount::control_path(&point);
        let mut args = crate::sshfs::arguments(&top, &point, control.as_deref());
        for option in &with.ssh_options {
            args.push("-o".into());
            args.push(option.into());
        }
        if let Some(config) = &with.ssh_config {
            args.push("-F".into());
            args.push(config.into());
        }
        // In the foreground, its manager waiting on it; nobody to answer.
        args.extend(["-f", "-o", "BatchMode=yes"].map(OsString::from));
        AtLogin {
            id: id_of(with.id(), &top),
            title: top.label(),
            program,
            args,
            point,
            folders: control
                .iter()
                .filter_map(|c| c.parent().map(Path::to_path_buf))
                .collect(),
            leftovers: control.into_iter().collect(),
            notifies: false,
            control: None,
        }
        .checked()
    }

    /// `self`, where every part is text a service manager's file holds:
    /// UTF-8, no control character but a tab or a new line.
    fn checked(self) -> anyhow::Result<AtLogin> {
        let paths = [&self.program, &self.point]
            .into_iter()
            .chain(&self.folders)
            .chain(&self.leftovers)
            .map(|path| path.as_os_str());
        for part in paths.chain(self.args.iter().map(OsString::as_os_str)) {
            let text = part.to_str().ok_or_else(|| {
                anyhow::anyhow!("“{}” is not text a service manager reads", part.display())
            })?;
            anyhow::ensure!(
                !text
                    .chars()
                    .any(|c| c.is_ascii_control() && !matches!(c, '\t' | '\n')),
                "“{}” holds a character a service manager does not read",
                text.escape_debug()
            );
        }
        Ok(self)
    }
}

/// rclone, the whole path of its configuration and `address`'s top, where
/// its mount may run at login: a remote of a configuration kept plain,
/// which names it. Reads the configuration.
fn rclone_checked(
    with: &crate::rclone::Rclone,
    address: &Address,
    cancel: &CancelToken,
) -> anyhow::Result<(PathBuf, PathBuf, Address)> {
    let top = address.top();
    anyhow::ensure!(
        top.scheme == Scheme::Rclone,
        "only a remote of rclone's configuration (rclone://name/) connects at login \
         through rclone: nobody is there to give a server's password"
    );
    if let Some(why) = with.unavailable() {
        anyhow::bail!("{why}");
    }
    let program = skwir_core::os_path::find_program("rclone")
        .ok_or_else(|| anyhow::anyhow!("rclone is not installed"))?;
    let config = std::path::absolute(crate::rclone_config::config_file(&program, with, cancel)?)?;
    let named = crate::rclone::Rclone::with_config(config.clone());
    if crate::rclone_config::encrypted(&program, &named, cancel)? {
        anyhow::bail!(
            "rclone's configuration is encrypted: at login, nobody is there to give its password"
        );
    }
    let mut listing = crate::rclone::command(&program, Some(&config));
    listing.args(["listremotes", "--ask-password=false"]);
    let listed = skwir_platform::process::run(&mut listing, cancel, Duration::from_secs(10))?;
    let name = format!("{}:", top.host);
    anyhow::ensure!(
        String::from_utf8_lossy(&listed)
            .lines()
            .any(|line| line.trim() == name),
        "rclone's configuration names no remote “{}”",
        top.host
    );
    Ok((program, config, top))
}

#[cfg(unix)]
fn rclone_on_unix(
    with: &crate::rclone::Rclone,
    program: PathBuf,
    config: PathBuf,
    top: &Address,
) -> anyhow::Result<AtLogin> {
    let remote = crate::rclone::remote_of(top)?;
    let point = point_of(with.id(), top)?;
    let mut args: Vec<OsString> = vec!["--config".into(), config.into()];
    args.extend(foreground(crate::rclone::mount_arguments(&remote, &point)));
    // Its remote control, as the connector gives it (`rclone_copy`).
    let (mut folders, mut leftovers) = (Vec::new(), Vec::new());
    if let Some(socket) = crate::rclone_copy::socket_for(&point) {
        let mut at = OsString::from("unix://");
        at.push(&socket);
        args.extend(["--rc".into(), "--rc-no-auth".into(), "--rc-addr".into(), at]);
        folders.extend(socket.parent().map(Path::to_path_buf));
        leftovers.push(socket);
    }
    AtLogin {
        id: id_of(with.id(), top),
        title: top.label(),
        program,
        args,
        point,
        folders,
        leftovers,
        notifies: true,
        control: None,
    }
    .checked()
}

/// On Windows, through `WinFsp`, its remote control behind a login of its
/// own, its port said in its log (`winfsp`). rclone's log kept from
/// growing where the installed rclone rotates it.
#[cfg(windows)]
fn rclone_on_windows(
    with: &crate::rclone::Rclone,
    program: PathBuf,
    config: PathBuf,
    top: &Address,
    cancel: &CancelToken,
) -> anyhow::Result<AtLogin> {
    let remote = crate::rclone::remote_of(top)?;
    let point = point_of(with.id(), top)?;
    let mut flags = crate::rclone::command(&program, None);
    flags.args(["help", "flags"]);
    let rotates = skwir_platform::process::run(&mut flags, cancel, Duration::from_secs(10))
        .is_ok_and(|said| String::from_utf8_lossy(&said).contains("--log-file-max-size"));
    let mut args: Vec<OsString> = vec!["--config".into(), config.into()];
    args.extend(
        crate::winfsp::at_login_arguments(&remote, &point, rotates)
            .ok_or_else(|| anyhow::anyhow!("there is no folder to mount in"))?,
    );
    AtLogin {
        id: id_of(with.id(), top),
        title: top.label(),
        program,
        args,
        folders: point.parent().map(|f| f.join(".rc")).into_iter().collect(),
        point,
        leftovers: Vec::new(),
        notifies: false,
        control: Some(crate::rc::Login::made()?),
    }
    .checked()
}

/// `rclone mount`'s arguments as Skwir gives them, less what sends it into
/// the background: its manager waits on it.
#[cfg(unix)]
fn foreground(args: Vec<OsString>) -> Vec<OsString> {
    let mut kept = Vec::with_capacity(args.len());
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--daemon-wait" | "--daemon-timeout") => {
                args.next();
            }
            Some(flag) if flag.starts_with("--daemon") => {}
            _ => kept.push(arg),
        }
    }
    kept
}

/// Whether ssh logs in to `top` asking nothing, as the unit's will at
/// login (`BatchMode`): once, now, with sshfs's options, sharing no
/// connection, asking the server for SFTP as sshfs does.
fn logs_in_alone(
    with: &crate::sshfs::Sshfs,
    top: &Address,
    cancel: &CancelToken,
) -> anyhow::Result<()> {
    let ssh = skwir_core::os_path::find_program("ssh")
        .ok_or_else(|| anyhow::anyhow!("ssh is not installed"))?;
    let mut command = Command::new(ssh);
    command.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=20",
        "-o",
        "ControlPath=none",
    ]);
    for option in &with.ssh_options {
        command.arg("-o").arg(option);
    }
    if let Some(config) = &with.ssh_config {
        command.arg("-F").arg(config);
    }
    if let Some(port) = top.port {
        command.arg("-p").arg(port.to_string());
    }
    let mut destination = String::new();
    if let Some(user) = &top.user {
        destination.push_str(user);
        destination.push('@');
    }
    destination.push_str(top.bare_host());
    command.arg("-s").arg(destination).arg("sftp");
    let finished =
        skwir_platform::process::run_to_exit(&mut command, None, cancel, Duration::from_secs(60))?;
    if finished.status.success() {
        return Ok(());
    }
    let said = finished.stderr;
    if said.contains("Permission denied") {
        anyhow::bail!(
            "the server wants a password or a key's passphrase: at login, nobody is there \
             to give it (a key ssh's agent holds, or one with no passphrase, logs in alone)"
        );
    }
    if said.contains("Host key verification failed") || said.contains("host key is known") {
        anyhow::bail!("ssh does not know the server's identity yet: connect to it once first");
    }
    anyhow::bail!(
        "it was not reached, to know it logs in alone: {}",
        crate::sshfs::words(&said)
    )
}

/// The programs a unit runs beside its mount's, found on `PATH` as it is
/// written: systemd looks for none there.
struct Helpers {
    mkdir: PathBuf,
    rm: PathBuf,
    rmdir: PathBuf,
    /// What lets go of a FUSE mount: fusermount3, else fusermount.
    unmount: PathBuf,
}

impl Helpers {
    fn found() -> Helpers {
        let find = |names: &[&str]| {
            names
                .iter()
                .find_map(skwir_core::os_path::find_program)
                .unwrap_or_else(|| PathBuf::from(names[0]))
        };
        Helpers {
            mkdir: find(&["mkdir"]),
            rm: find(&["rm"]),
            rmdir: find(&["rmdir"]),
            unmount: find(&["fusermount3", "fusermount"]),
        }
    }
}

/// What lets go of what a run left, run by `/bin/sh` with the remover as
/// `$0`, then the point, the unmounter, the folder remover and the
/// leftovers. The point is removed where it is an empty folder. Where it
/// then still answers (a mount alive: Skwir's, never one this unit's
/// program left, which is gone), nothing else is done; else a mount whose
/// program is gone is let go of, lazily, the point removed, and the
/// leftovers.
const GONE: &str = r#""$3" "$1" 2>/dev/null; [ -e "$1" ] && exit 0; "$2" -uz "$1" 2>/dev/null; "$3" "$1" 2>/dev/null; shift 3; exec "$0" -f -- "$@""#;

/// The systemd user unit that runs `at` at login (`systemd.service`): its
/// mount command in the foreground (`Type=notify` where it tells once
/// mounted, else `exec`), what a run that died left let go of first, the
/// folders made, then on stopping its mount let go of and its point
/// removed; started again when it fails, wanted by `default.target`.
/// Every word is quoted as systemd reads it back exactly (`exec_word`).
///
/// Most user managers have no `network-online.target`: wanting it orders
/// the unit after the network where one does, and failing to reach its
/// server, it tries again ten seconds later.
pub fn systemd_unit(at: &AtLogin) -> String {
    let helpers = Helpers::found();
    let point = OsString::from(&at.point);
    let mut gone: Vec<OsString> = vec![
        "-c".into(),
        GONE.into(),
        helpers.rm.into(),
        point.clone(),
        helpers.unmount.clone().into(),
        helpers.rmdir.into(),
    ];
    gone.extend(at.leftovers.iter().map(OsString::from));
    let gone = exec_line(Path::new("/bin/sh"), &gone);
    let mut made: Vec<OsString> = ["-p", "-m", "700", "--"].map(OsString::from).to_vec();
    made.push(point.clone());
    made.extend(at.folders.iter().map(OsString::from));
    let plain = unit_text(&at.title);
    let title = plain.replace('%', "%%");
    let mut unit = String::new();
    let _ = writeln!(
        unit,
        "# Made by Skwir: {plain} connects at login. Turned off in Skwir, it is removed.\n\
         [Unit]\n\
         Description=Connect {title} at login (Skwir)\n\
         Wants=network-online.target\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type={kind}\n\
         ExecStartPre=-{gone}\n\
         ExecStartPre={made}\n\
         ExecStart={start}\n\
         ExecStop=-{stop}\n\
         ExecStopPost=-{gone}\n\
         Restart=on-failure\n\
         RestartSec=10s\n\
         \n\
         [Install]\n\
         WantedBy=default.target",
        kind = if at.notifies { "notify" } else { "exec" },
        made = exec_line(&helpers.mkdir, &made),
        start = exec_line(&at.program, &at.args),
        stop = exec_line(&helpers.unmount, &["-u".into(), point]),
    );
    unit
}

/// `text` on one line of a unit: a control character as a space. A value
/// writes its `%` as `%%` (a specifier otherwise); a comment does not.
fn unit_text(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// `program` run with `args`, as a systemd command line. Through `/bin/sh`,
/// which runs it in its place, where systemd refuses its path (it holds a
/// quote, a backslash or a control character).
fn exec_line(program: &Path, args: &[OsString]) -> String {
    let program = program.to_string_lossy();
    let mut line = if program.contains(['"', '\'', '\\']) || program.chars().any(char::is_control) {
        format!(
            "/bin/sh -c {} {}",
            exec_word(r#"exec "$0" "$@""#, true),
            exec_word(&program, true)
        )
    } else {
        exec_word(&program, false)
    };
    for arg in args {
        line.push(' ');
        line.push_str(&exec_word(&arg.to_string_lossy(), true));
    }
    line
}

/// `word` as a word of a systemd command line (`systemd.syntax`'s
/// quoting): in double quotes, a backslash and a quote escaped, a control
/// character as a C escape, `%` as `%%` (a specifier otherwise). An
/// argument's `$` is `$$` (a variable otherwise); the program's path is
/// not substituted.
fn exec_word(word: &str, argument: bool) -> String {
    let mut out = String::with_capacity(word.len() + 2);
    out.push('"');
    for c in word.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '%' => out.push_str("%%"),
            '$' if argument => out.push_str("$$"),
            c if c.is_ascii_control() => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The systemd unit of `id`: `skwir-remote-rclone-rclone:--gdrive-.service`,
/// `skwir/remote/<id>` escaped as `systemd-escape` escapes (which reads it
/// back, `-u`), as systemd names a mount's unit by its folder. Past
/// systemd's 255 bytes, cut and told apart by a hash of the whole.
pub fn unit_name(id: &str) -> String {
    let escaped = escaped(&format!("skwir/remote/{id}"), "\\x", |b| {
        b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'.')
    });
    fitted(&escaped, "\\x", ".service", 255)
}

/// The launch agent's label of `id`, its file's name too (`<label>.plist`):
/// `org.skwir.skwir.remote.` and the id escaped as a unit's name is.
pub fn agent_label(id: &str) -> String {
    let escaped = escaped(id, "\\x", |b| {
        b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'.')
    });
    fitted(&format!("org.skwir.skwir.remote.{escaped}"), "\\x", "", 249)
}

/// The logon task's name of `id`, in Skwir's folder of the Task Scheduler
/// (`Skwir\remote-rclone-rclone_x3a--gdrive-`): escaped as a unit's name
/// is, its escapes marked `_x` and `:` escaped too, which a task's name
/// cannot hold.
pub fn task_name(id: &str) -> String {
    let escaped = escaped(id, "_x", |b| b.is_ascii_alphanumeric() || b == b'.');
    format!(
        "Skwir\\{}",
        fitted(&format!("remote-{escaped}"), "_x", "", 200)
    )
}

/// `text` escaped as systemd escapes a name (`systemd-escape`): `/` as `-`,
/// a byte `kept` takes as it is (but a `.` first), any other as `mark` and
/// its two hexadecimal digits.
fn escaped(text: &str, mark: &str, kept: fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    for (at, byte) in text.bytes().enumerate() {
        match byte {
            b'/' => out.push('-'),
            b'.' if at == 0 => {
                let _ = write!(out, "{mark}{byte:02x}");
            }
            byte if kept(byte) => out.push(char::from(byte)),
            byte => {
                let _ = write!(out, "{mark}{byte:02x}");
            }
        }
    }
    out
}

/// `name` and `suffix`, `most` bytes at most: a longer name cut where no
/// escape (`mark` and two digits) is cut in two, a hash of the whole after.
fn fitted(name: &str, mark: &str, suffix: &str, most: usize) -> String {
    if name.len() + suffix.len() <= most {
        return format!("{name}{suffix}");
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    let hash = format!("-{hash:016x}");
    let mut cut = most - suffix.len() - hash.len();
    let escape = mark.len() + 2;
    if let Some(start) =
        (cut.saturating_sub(escape - 1)..cut).find(|&at| name[at..].starts_with(mark))
    {
        cut = start;
    }
    format!("{}{hash}{suffix}", &name[..cut])
}

/// What its manager says it is: "Connect gdrive at login (Skwir)".
fn description(at: &AtLogin) -> String {
    format!("Connect {} at login (Skwir)", at.title)
}

/// The launch agent that runs `at` at login (`launchd.plist`): through
/// `/bin/sh`, which lets go of what a run that died left, makes the
/// folders, then runs the mount command in its place, its arguments the
/// script's; at load, again when it fails (`KeepAlive`), ten seconds
/// apart at least.
pub fn launch_agent(at: &AtLogin) -> String {
    let mut arguments: Vec<String> = vec!["/bin/sh".into(), "-c".into(), agent_script(at)];
    arguments.push(at.program.to_string_lossy().into_owned());
    arguments.extend(at.args.iter().map(|a| a.to_string_lossy().into_owned()));
    let mut plist = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <!-- Made by Skwir: its remote connects at login. Turned off in Skwir, it is removed. -->\n\
         <plist version=\"1.0\">\n<dict>\n",
    );
    let _ = writeln!(
        plist,
        "  <key>Label</key>\n  <string>{}</string>",
        xml_text(&agent_label(&at.id))
    );
    plist.push_str("  <key>ProgramArguments</key>\n  <array>\n");
    for argument in &arguments {
        let _ = writeln!(plist, "    <string>{}</string>", xml_text(argument));
    }
    plist.push_str(
        "  </array>\n\
         \x20 <key>RunAtLoad</key>\n  <true/>\n\
         \x20 <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>\n\
         \x20 <key>ThrottleInterval</key>\n  <integer>10</integer>\n\
         </dict>\n</plist>\n",
    );
    plist
}

/// The script a launch agent runs `at` through: macOS's own programs, the
/// paths quoted for the shell, the mount command its arguments.
fn agent_script(at: &AtLogin) -> String {
    let quoted = |path: &Path| sh_quoted(&path.to_string_lossy());
    // As `GONE`: an empty point removed, then one that does not answer let
    // go of, with what its run left.
    let mut script = format!(
        "p={}; /bin/rmdir \"$p\" 2>/dev/null; [ -e \"$p\" ] || \
         {{ /sbin/umount -f \"$p\"; /bin/rmdir \"$p\"",
        quoted(&at.point)
    );
    if !at.leftovers.is_empty() {
        script.push_str("; /bin/rm -f --");
        for leftover in &at.leftovers {
            script.push(' ');
            script.push_str(&quoted(leftover));
        }
    }
    script.push_str("; } 2>/dev/null; /bin/mkdir -p -m 700 -- \"$p\"");
    for folder in &at.folders {
        script.push(' ');
        script.push_str(&quoted(folder));
    }
    script.push_str(" && exec \"$0\" \"$@\"");
    script
}

/// `text` as one word of the shell: in single quotes, each of its own
/// written `'\''`.
fn sh_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The Task Scheduler's task that runs `at` when `user` (`DOMAIN\name`)
/// logs in: hidden, no time limit, started again when it fails, never a
/// second at once. Registered by `schtasks /Create /XML`.
pub fn logon_task(at: &AtLogin, user: &str) -> String {
    let user = xml_text(user);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>\n\
         <!-- Made by Skwir: its remote connects at login. Turned off in Skwir, it is removed. -->\n\
         <Task version=\"1.2\" xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">\n\
         \x20 <RegistrationInfo>\n\
         \x20   <Description>{description}</Description>\n\
         \x20 </RegistrationInfo>\n\
         \x20 <Triggers>\n\
         \x20   <LogonTrigger>\n\
         \x20     <Enabled>true</Enabled>\n\
         \x20     <UserId>{user}</UserId>\n\
         \x20   </LogonTrigger>\n\
         \x20 </Triggers>\n\
         \x20 <Principals>\n\
         \x20   <Principal id=\"Author\">\n\
         \x20     <UserId>{user}</UserId>\n\
         \x20     <LogonType>InteractiveToken</LogonType>\n\
         \x20     <RunLevel>LeastPrivilege</RunLevel>\n\
         \x20   </Principal>\n\
         \x20 </Principals>\n\
         \x20 <Settings>\n\
         \x20   <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>\n\
         \x20   <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>\n\
         \x20   <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>\n\
         \x20   <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>\n\
         \x20   <Hidden>true</Hidden>\n\
         \x20   <RestartOnFailure>\n\
         \x20     <Interval>PT1M</Interval>\n\
         \x20     <Count>999</Count>\n\
         \x20   </RestartOnFailure>\n\
         \x20   <Enabled>true</Enabled>\n\
         \x20 </Settings>\n\
         \x20 <Actions Context=\"Author\">\n\
         \x20   <Exec>\n\
         \x20     <Command>{command}</Command>\n\
         \x20     <Arguments>{arguments}</Arguments>\n\
         \x20   </Exec>\n\
         \x20 </Actions>\n\
         </Task>\n",
        description = xml_text(&description(at)),
        command = xml_text(&at.program.to_string_lossy()),
        arguments = xml_text(&windows_command_line(&at.args)),
    )
}

/// `args` as one command line Windows splits back into them (the C
/// runtime's rules, `CommandLineToArgvW`'s): a word with no space, tab or
/// quote as it is; any other in quotes, a quote and the backslashes before
/// it escaped, and those before the closing quote doubled.
fn windows_command_line(args: &[OsString]) -> String {
    let mut line = String::new();
    for arg in args {
        let arg = arg.to_string_lossy();
        if !line.is_empty() {
            line.push(' ');
        }
        if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\u{b}', '"']) {
            line.push_str(&arg);
            continue;
        }
        line.push('"');
        let mut backslashes = 0;
        for c in arg.chars() {
            match c {
                '\\' => backslashes += 1,
                '"' => {
                    line.push_str(&"\\".repeat(backslashes * 2 + 1));
                    line.push('"');
                    backslashes = 0;
                }
                c => {
                    line.push_str(&"\\".repeat(backslashes));
                    line.push(c);
                    backslashes = 0;
                }
            }
        }
        line.push_str(&"\\".repeat(backslashes * 2));
        line.push('"');
    }
    line
}

/// `text` as XML's text or an attribute's value holds it: its markup
/// escaped, a control character as a character reference (a parser keeps
/// a tab and a new line so, and drops none).
fn xml_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c if c.is_control() => {
                let _ = write!(out, "&#x{:x};", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out
}

/// Whether something is mounted at `point` now: a mount Skwir made there,
/// or the unit's own.
fn mounted(point: &Path) -> bool {
    skwir_core::fs::reach::of(point).is_some_and(|remote| remote.point == point)
}

/// The service manager entries go to, and the program it is asked
/// through: the user's own ([`Manager::user`]), or one a test names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manager {
    /// Where its entries' files are: systemd's
    /// `$XDG_CONFIG_HOME/systemd/user`, launchd's `~/Library/LaunchAgents`;
    /// on Windows, where a task's file is written for `schtasks` to read.
    pub folder: PathBuf,
    /// What it is asked through: `systemctl`, `launchctl`, `schtasks`.
    pub program: PathBuf,
    /// On Linux, the runtime folder `systemctl --user` finds its manager
    /// in, its environment holding nothing else: a manager of a test's
    /// own. `None`: as Skwir's environment says, the user's.
    pub runtime: Option<PathBuf>,
}

/// Set `at` up to connect at each login, and connect it now where nothing
/// is mounted at its point: with the user's own service manager
/// ([`Manager::install`]).
///
/// # Errors
/// As [`Manager::install`].
pub fn install(at: &AtLogin, cancel: &CancelToken) -> anyhow::Result<()> {
    Manager::user()?.install(at, cancel)
}

/// Stop connecting `id` at login ([`Manager::uninstall`]).
///
/// # Errors
/// As [`Manager::uninstall`].
pub fn uninstall(id: &str, point: &Path, cancel: &CancelToken) -> anyhow::Result<()> {
    Manager::user()?.uninstall(id, point, cancel)
}

/// Whether `id` connects at login ([`Manager::installed`]).
pub fn installed(id: &str) -> bool {
    Manager::user().is_ok_and(|manager| manager.installed(id))
}

impl Manager {
    /// The user's own: systemd's user manager, launchd's agents of the
    /// user's session, the Task Scheduler.
    ///
    /// # Errors
    /// Where the system has none Skwir asks, or its program is missing.
    pub fn user() -> anyhow::Result<Manager> {
        #[cfg(target_os = "linux")]
        {
            let config = skwir_core::xdg::config_home()
                .ok_or_else(|| anyhow::anyhow!("there is no configuration folder"))?;
            Ok(Manager {
                folder: config.join("systemd").join("user"),
                program: skwir_core::os_path::find_program("systemctl").ok_or_else(|| {
                    anyhow::anyhow!("systemd is not here to connect at login (no systemctl)")
                })?,
                runtime: None,
            })
        }
        #[cfg(target_os = "macos")]
        {
            let home = skwir_core::os_path::home()
                .ok_or_else(|| anyhow::anyhow!("there is no home folder"))?;
            Ok(Manager {
                folder: home.join("Library").join("LaunchAgents"),
                program: PathBuf::from("/bin/launchctl"),
                runtime: None,
            })
        }
        #[cfg(windows)]
        {
            Ok(Manager {
                folder: std::env::temp_dir(),
                program: skwir_core::os_path::find_program("schtasks")
                    .ok_or_else(|| anyhow::anyhow!("the Task Scheduler's schtasks is missing"))?,
                runtime: None,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            Err(skwir_core::system::not_yet("Connecting at login").into())
        }
    }

    /// Set `at` up to connect at each login, then connect it now where
    /// nothing is mounted at its point (systemd waits until it is mounted,
    /// or failed); where something is (Skwir's own mount), it takes over at
    /// the next login. Writes files and waits on its manager: on a worker.
    ///
    /// # Errors
    /// What its manager said: then nothing is left set up.
    pub fn install(&self, at: &AtLogin, cancel: &CancelToken) -> anyhow::Result<()> {
        #[cfg(target_os = "linux")]
        {
            self.install_unit(at, cancel)
        }
        #[cfg(target_os = "macos")]
        {
            self.install_agent(at, cancel)
        }
        #[cfg(windows)]
        {
            self.install_task(at, cancel)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (at, cancel);
            Err(skwir_core::system::not_yet("Connecting at login").into())
        }
    }

    /// Stop connecting `id` at login: its entry removed and forgotten. A
    /// mount it carries at `point` stays until Eject or the session's end,
    /// as a mount Skwir made: its manager forgot the entry's own commands
    /// (letting go of its point, starting it again) with its file. Where
    /// none is, it is stopped too (a run failing again and again).
    ///
    /// # Errors
    /// What its manager said.
    pub fn uninstall(&self, id: &str, point: &Path, cancel: &CancelToken) -> anyhow::Result<()> {
        #[cfg(target_os = "linux")]
        {
            self.uninstall_unit(&unit_name(id), mounted(point), cancel)
        }
        #[cfg(target_os = "macos")]
        {
            self.uninstall_agent(&agent_label(id), mounted(point), cancel)
        }
        #[cfg(windows)]
        {
            let keep = mounted(point);
            self.uninstall_task(&task_name(id), keep, cancel)?;
            // A mount it carries is let go of through its remote control,
            // whose login goes with it (Eject).
            if !keep {
                crate::winfsp::forget_at_login(point);
            }
            Ok(())
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = (id, point, cancel);
            Err(skwir_core::system::not_yet("Connecting at login").into())
        }
    }

    /// Whether `id` connects at login: its file is there, enabled (on
    /// Linux, wanted by `default.target`), or its task registered (asked of
    /// `schtasks`: briefly blocks).
    pub fn installed(&self, id: &str) -> bool {
        #[cfg(target_os = "linux")]
        {
            let name = unit_name(id);
            self.folder.join(&name).is_file()
                && self
                    .folder
                    .join("default.target.wants")
                    .join(&name)
                    .symlink_metadata()
                    .is_ok()
        }
        #[cfg(target_os = "macos")]
        {
            self.folder
                .join(format!("{}.plist", agent_label(id)))
                .is_file()
        }
        #[cfg(windows)]
        {
            let mut command = Command::new(&self.program);
            command.args(["/Query", "/TN"]).arg(task_name(id));
            skwir_platform::process::run_to_exit(
                &mut command,
                None,
                &CancelToken::new(),
                Duration::from_secs(10),
            )
            .is_ok_and(|finished| finished.status.success())
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        {
            let _ = id;
            false
        }
    }

    /// Run its program with `args`, its words when it fails.
    fn ask(&self, args: Vec<OsString>, cancel: &CancelToken) -> anyhow::Result<()> {
        let mut command = Command::new(&self.program);
        command.args(args);
        if let Some(runtime) = &self.runtime {
            command.env_clear().env("XDG_RUNTIME_DIR", runtime);
        }
        let finished = skwir_platform::process::run_to_exit(&mut command, None, cancel, PATIENCE)?;
        if finished.status.success() {
            return Ok(());
        }
        let said = finished.stderr.trim();
        anyhow::bail!(
            "{}",
            said.lines()
                .next_back()
                .map_or_else(|| format!("it ended ({})", finished.status), str::to_owned)
        )
    }
}

#[cfg(target_os = "linux")]
impl Manager {
    fn systemctl(&self, args: &[&str], cancel: &CancelToken) -> anyhow::Result<()> {
        let args = std::iter::once("--user")
            .chain(args.iter().copied())
            .map(OsString::from)
            .collect();
        self.ask(args, cancel)
    }

    fn install_unit(&self, at: &AtLogin, cancel: &CancelToken) -> anyhow::Result<()> {
        let name = unit_name(&at.id);
        std::fs::create_dir_all(&self.folder)?;
        let unit = systemd_unit(at);
        skwir_platform::file::rewrite(&self.folder.join(&name), "the login unit", |_| {
            Some((unit, ()))
        })?;
        let set_up = self
            .systemctl(&["daemon-reload"], cancel)
            .and_then(|()| self.systemctl(&["enable", "--quiet", name.as_str()], cancel));
        let started = set_up.and_then(|()| {
            if mounted(&at.point) {
                return Ok(());
            }
            self.systemctl(&["start", name.as_str()], cancel)
                .map_err(|error| {
                    anyhow::anyhow!("{error:#} (journalctl --user -u {name} says why)")
                })
        });
        if let Err(error) = started {
            let _ = self.uninstall_unit(&name, false, &CancelToken::new());
            return Err(error);
        }
        Ok(())
    }

    fn uninstall_unit(&self, name: &str, keep: bool, cancel: &CancelToken) -> anyhow::Result<()> {
        let file = self.folder.join(name);
        let there = file.exists();
        let mut disable = vec!["disable", "--quiet"];
        if !keep {
            disable.push("--now");
        }
        disable.push(name);
        let disabled = self.systemctl(&disable, cancel);
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        self.systemctl(&["daemon-reload"], cancel)?;
        // One never written is nothing to disable.
        if there { disabled } else { Ok(()) }
    }
}

#[cfg(target_os = "macos")]
impl Manager {
    /// The user's session in launchd: `gui/<uid>`.
    fn domain(cancel: &CancelToken) -> anyhow::Result<String> {
        let mut id = Command::new("/usr/bin/id");
        id.arg("-u");
        let uid = skwir_platform::process::run(&mut id, cancel, Duration::from_secs(10))?;
        Ok(format!("gui/{}", String::from_utf8_lossy(&uid).trim()))
    }

    fn install_agent(&self, at: &AtLogin, cancel: &CancelToken) -> anyhow::Result<()> {
        let label = agent_label(&at.id);
        let file = self.folder.join(format!("{label}.plist"));
        std::fs::create_dir_all(&self.folder)?;
        let agent = launch_agent(at);
        skwir_platform::file::rewrite(&file, "the login agent", |_| Some((agent, ())))?;
        if mounted(&at.point) {
            return Ok(());
        }
        let started = Manager::domain(cancel).and_then(|domain| {
            // One loaded already (failing again and again) is loaded anew.
            let _ = self.ask(
                vec!["bootout".into(), format!("{domain}/{label}").into()],
                cancel,
            );
            self.ask(
                vec!["bootstrap".into(), domain.into(), file.clone().into()],
                cancel,
            )
        });
        if let Err(error) = started {
            let _ = std::fs::remove_file(&file);
            return Err(error);
        }
        Ok(())
    }

    fn uninstall_agent(&self, label: &str, keep: bool, cancel: &CancelToken) -> anyhow::Result<()> {
        if !keep {
            let domain = Manager::domain(cancel)?;
            let _ = self.ask(
                vec!["bootout".into(), format!("{domain}/{label}").into()],
                cancel,
            );
        }
        match std::fs::remove_file(self.folder.join(format!("{label}.plist"))) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(windows)]
impl Manager {
    fn install_task(&self, at: &AtLogin, cancel: &CancelToken) -> anyhow::Result<()> {
        let name = task_name(&at.id);
        let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
            (Ok(domain), Ok(user)) => format!("{domain}\\{user}"),
            (_, Ok(user)) => user,
            _ => anyhow::bail!("Windows does not say who the user is (USERNAME)"),
        };
        // schtasks reads the task as UTF-16, as its declaration says.
        let mut bytes = vec![0xff, 0xfe];
        for unit in logon_task(at, &user).encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        std::fs::create_dir_all(&self.folder)?;
        let file = self.folder.join(format!(
            "skwir-task-{}-{:x}.xml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::write(&file, bytes)?;
        // Its remote control's login, which its rclone reads as it starts.
        if let Some(login) = &at.control {
            crate::winfsp::keep_at_login(&at.point, login)?;
        }
        let created = self.ask(
            vec![
                "/Create".into(),
                "/XML".into(),
                file.clone().into(),
                "/TN".into(),
                name.clone().into(),
                "/F".into(),
            ],
            cancel,
        );
        let _ = std::fs::remove_file(&file);
        if created.is_err() && at.control.is_some() && !mounted(&at.point) {
            crate::winfsp::forget_at_login(&at.point);
        }
        created?;
        if mounted(&at.point) {
            return Ok(());
        }
        if let Err(error) = self.ask(
            vec!["/Run".into(), "/TN".into(), name.clone().into()],
            cancel,
        ) {
            let _ = self.uninstall_task(&name, false, &CancelToken::new());
            crate::winfsp::forget_at_login(&at.point);
            return Err(error);
        }
        Ok(())
    }

    fn uninstall_task(&self, name: &str, keep: bool, cancel: &CancelToken) -> anyhow::Result<()> {
        if !keep {
            let _ = self.ask(vec!["/End".into(), "/TN".into(), name.into()], cancel);
        }
        self.ask(
            vec!["/Delete".into(), "/TN".into(), name.into(), "/F".into()],
            cancel,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words that read differently in every grammar a unit, a plist and a
    /// task pass through.
    const HOSTILE: &[&str] = &[
        "plain",
        "two words",
        "it's",
        "say \"hi\"",
        "back\\slash",
        "trailing\\",
        "\\\"",
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
        "@at",
        "&amp; <b>",
        "\\x41",
    ];

    fn hostile_at_login() -> AtLogin {
        AtLogin {
            id: "rclone/rclone://my.drive+2/".to_owned(),
            title: "my 100% drive".to_owned(),
            program: PathBuf::from("/opt/my tools/é/rclone"),
            args: HOSTILE.iter().map(OsString::from).collect(),
            point: PathBuf::from("/run/user/1000/skwir/remote/rclone rclone:%2F%2Fd%2F $x 'q'"),
            folders: vec![PathBuf::from("/run/user/1000/skwir/rc")],
            leftovers: vec![PathBuf::from("/run/user/1000/skwir/rc/0123.sock")],
            notifies: true,
            control: None,
        }
    }

    /// The words systemd reads from a command line in a unit: quoted
    /// (double or single quotes, C escapes), then `%%` as `%`, then, in an
    /// argument, `$$` as `$` (systemd.syntax, systemd.service). `None` for
    /// what it would read otherwise (a specifier, a variable, a lone `;`).
    fn systemd_words(line: &str) -> Option<Vec<String>> {
        let line = line.trim_start_matches(['-', '@', ':', '+', '!']);
        let mut words = Vec::new();
        let mut chars = line.chars().peekable();
        loop {
            while chars.next_if(|c| c.is_whitespace()).is_some() {}
            let Some(&first) = chars.peek() else { break };
            let mut word = String::new();
            let mut quote = None;
            if first == ';' {
                return None;
            }
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
        Some(read)
    }

    /// The value of each line of `unit` whose key is `key`.
    fn values<'a>(unit: &'a str, key: &str) -> Vec<&'a str> {
        unit.lines()
            .filter_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .collect()
    }

    #[test]
    fn a_login_unit_runs_the_mount_command_word_for_word() {
        let at = hostile_at_login();
        let unit = systemd_unit(&at);
        let start = values(&unit, "ExecStart");
        assert_eq!(start.len(), 1, "{unit}");
        let words = systemd_words(start[0]).unwrap();
        assert_eq!(words[0], "/opt/my tools/é/rclone");
        assert_eq!(&words[1..], HOSTILE, "{}", start[0]);
        assert_eq!(values(&unit, "Type"), ["notify"]);
        assert!(start[0].contains("\"100%%\"") && start[0].contains("\"$$HOME\""));
        // What it stops with lets go of the point it mounted.
        let stop = systemd_words(values(&unit, "ExecStop")[0]).unwrap();
        assert_eq!(stop[1..], ["-u".to_owned(), at.point.display().to_string()]);
        assert!(values(&unit, "ExecStop")[0].starts_with('-'));
        let made = systemd_words(values(&unit, "ExecStartPre")[1]).unwrap();
        assert_eq!(
            made[1..],
            [
                "-p",
                "-m",
                "700",
                "--",
                at.point.to_str().unwrap(),
                "/run/user/1000/skwir/rc"
            ]
        );
        // What a run that died left: the script, then its words.
        let gone = systemd_words(values(&unit, "ExecStartPre")[0]).unwrap();
        assert_eq!(gone[..3], ["/bin/sh", "-c", GONE]);
        assert_eq!(gone[4], at.point.to_string_lossy());
        assert_eq!(gone[7], "/run/user/1000/skwir/rc/0123.sock");
        assert_eq!(
            values(&unit, "ExecStopPost"),
            values(&unit, "ExecStartPre")[..1]
        );
        assert_eq!(
            values(&unit, "Description"),
            ["Connect my 100%% drive at login (Skwir)"]
        );
        assert_eq!(values(&unit, "WantedBy"), ["default.target"]);
    }

    /// systemd refuses a program's path holding a quote or a backslash: the
    /// shell runs it in its place, its path then an argument.
    #[test]
    fn a_program_systemd_refuses_runs_through_the_shell() {
        let mut at = hostile_at_login();
        at.program = PathBuf::from("/home/o'brien/bin/rc\\lone \"x\"");
        at.notifies = false;
        let unit = systemd_unit(&at);
        let words = systemd_words(values(&unit, "ExecStart")[0]).unwrap();
        assert_eq!(
            words[..4],
            [
                "/bin/sh",
                "-c",
                "exec \"$0\" \"$@\"",
                "/home/o'brien/bin/rc\\lone \"x\""
            ]
        );
        assert_eq!(&words[4..], HOSTILE);
        assert_eq!(values(&unit, "Type"), ["exec"]);
    }

    #[test]
    fn a_unit_is_named_as_systemd_escapes_its_path() {
        assert_eq!(
            unit_name("rclone/rclone://gdrive/"),
            "skwir-remote-rclone-rclone:--gdrive-.service"
        );
        assert_eq!(
            unit_name("sshfs/sftp://me@example.com:2222/"),
            "skwir-remote-sshfs-sftp:--me\\x40example.com:2222-.service"
        );
        let odd = unit_name("x/.a b-c\\d%e$f'g\"h é");
        assert_eq!(
            odd,
            "skwir-remote-x-.a\\x20b\\x2dc\\x5cd\\x25e\\x24f\\x27g\\x22h\\x20\\xc3\\xa9.service"
        );
        // Long ones are cut, told apart by a hash, never an escape cut.
        let long = |end: &str| unit_name(&format!("sshfs/sftp://{}{end}/", "é".repeat(80)));
        let (one, two) = (long("a"), long("b"));
        assert!(one.len() <= 255 && two.len() <= 255, "{one}");
        assert_ne!(one, two);
        assert!(one.ends_with(".service"));
        let kept = one.trim_end_matches(".service");
        let kept = &kept[..kept.len() - 17];
        assert!(
            kept.rsplit("\\x")
                .next()
                .is_some_and(|last| last.len() >= 2),
            "{kept}"
        );
        assert!(
            one.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b":_.\\-".contains(&b))
        );
    }

    #[test]
    fn a_task_name_holds_nothing_windows_refuses() {
        assert_eq!(
            task_name("rclone/rclone://gdrive/"),
            "Skwir\\remote-rclone-rclone_x3a--gdrive-"
        );
        let name = task_name("sshfs/sftp://o'brien@h:22/\\x*?<>|\"");
        let (folder, task) = name.split_once('\\').unwrap();
        assert_eq!(folder, "Skwir");
        assert!(
            task.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "{task}"
        );
        assert_ne!(task_name("a/b_c"), task_name("a/b:c"));
        assert!(task_name(&"é".repeat(300)).len() <= "Skwir\\".len() + 200);
        assert_eq!(
            agent_label("rclone/rclone://gdrive/"),
            "org.skwir.skwir.remote.rclone-rclone:--gdrive-"
        );
    }

    #[test]
    fn a_launch_agent_holds_the_mount_command_word_for_word() {
        let at = hostile_at_login();
        let plist = launch_agent(&at);
        let document = roxmltree::Document::parse_with_options(
            &plist,
            roxmltree::ParsingOptions {
                allow_dtd: true,
                ..roxmltree::ParsingOptions::default()
            },
        )
        .unwrap();
        let dict = document
            .root_element()
            .children()
            .find(|n| n.has_tag_name("dict"))
            .unwrap();
        let entries: Vec<_> = dict
            .children()
            .filter(roxmltree::Node::is_element)
            .collect();
        let value = |key: &str| {
            let at = entries
                .iter()
                .position(|n| n.has_tag_name("key") && n.text() == Some(key))
                .unwrap();
            entries[at + 1]
        };
        assert_eq!(
            value("Label").text(),
            Some("org.skwir.skwir.remote.rclone-rclone:--my.drive\\x2b2-")
        );
        let arguments: Vec<String> = value("ProgramArguments")
            .children()
            .filter(|n| n.has_tag_name("string"))
            .map(|n| n.text().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(arguments[..2], ["/bin/sh", "-c"]);
        assert_eq!(arguments[3], "/opt/my tools/é/rclone");
        assert_eq!(&arguments[4..], HOSTILE);
        // The point is one word of the shell, whatever it holds.
        assert!(
            arguments[2].starts_with(
                "p='/run/user/1000/skwir/remote/rclone rclone:%2F%2Fd%2F $x '\\''q'\\'''; "
            ),
            "{}",
            arguments[2]
        );
        assert!(arguments[2].ends_with("&& exec \"$0\" \"$@\""));
        assert!(value("RunAtLoad").has_tag_name("true"));
    }

    /// How Windows splits a command line into words (the C runtime's
    /// rules), as a program the task starts reads it.
    fn windows_words(line: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut chars = line.chars().peekable();
        loop {
            while chars.next_if(|c| *c == ' ' || *c == '\t').is_some() {}
            if chars.peek().is_none() {
                break;
            }
            let (mut word, mut quoted) = (String::new(), false);
            loop {
                let mut backslashes = 0;
                while chars.next_if_eq(&'\\').is_some() {
                    backslashes += 1;
                }
                match chars.peek() {
                    Some('"') => {
                        word.push_str(&"\\".repeat(backslashes / 2));
                        chars.next();
                        if backslashes % 2 == 1 {
                            word.push('"');
                        } else if quoted && chars.peek() == Some(&'"') {
                            chars.next();
                            word.push('"');
                        } else {
                            quoted = !quoted;
                        }
                    }
                    Some(&c) if !quoted && (c == ' ' || c == '\t') => {
                        word.push_str(&"\\".repeat(backslashes));
                        break;
                    }
                    Some(&c) => {
                        word.push_str(&"\\".repeat(backslashes));
                        word.push(c);
                        chars.next();
                    }
                    None => {
                        word.push_str(&"\\".repeat(backslashes));
                        break;
                    }
                }
            }
            words.push(word);
        }
        words
    }

    #[test]
    fn a_logon_task_holds_the_mount_command_word_for_word() {
        let mut at = hostile_at_login();
        at.program = PathBuf::from(r"C:\Program Files\rclone & co\rclone.exe");
        let mut args: Vec<OsString> = HOSTILE.iter().map(OsString::from).collect();
        args.push("".into());
        args.push(r"C:\trailing\".into());
        at.args = args.clone();
        let task = logon_task(&at, r"WORK\o'brien & co");
        let document = roxmltree::Document::parse(&task).unwrap();
        let text = |tag: &str| {
            document
                .descendants()
                .find(|n| n.has_tag_name(tag))
                .and_then(|n| n.text())
                .unwrap_or_default()
                .to_owned()
        };
        assert_eq!(text("Command"), r"C:\Program Files\rclone & co\rclone.exe");
        let words = windows_words(&text("Arguments"));
        let wanted: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(words, wanted);
        let logon = document
            .descendants()
            .find(|n| n.has_tag_name("LogonTrigger"))
            .unwrap();
        assert_eq!(
            logon
                .children()
                .find(|n| n.has_tag_name("UserId"))
                .and_then(|n| n.text()),
            Some(r"WORK\o'brien & co")
        );
        assert_eq!(text("ExecutionTimeLimit"), "PT0S");
        assert_eq!(text("Hidden"), "true");
        assert_eq!(
            text("Description"),
            "Connect my 100% drive at login (Skwir)"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rclone_runs_in_the_foreground_at_login() {
        let given = crate::rclone::mount_arguments("gdrive:", Path::new("/m"));
        let kept = foreground(given.clone());
        assert!(
            !kept
                .iter()
                .any(|a| a.to_string_lossy().starts_with("--daemon"))
        );
        let wait = given.iter().position(|a| a == "--daemon-wait").unwrap();
        let mut wanted = given.clone();
        wanted.drain(wait..=wait + 1);
        wanted.retain(|a| a != "--daemon");
        assert_eq!(kept, wanted, "all else as Skwir mounts it");
        assert_eq!(
            foreground(vec!["--daemon-wait=1m".into(), "x".into()]),
            ["x"]
        );
    }

    #[test]
    fn what_a_manager_cannot_read_is_refused() {
        let mut at = hostile_at_login();
        assert!(at.clone().checked().is_ok());
        at.args.push("bell\u{7}".into());
        assert!(at.checked().is_err());
    }
}
