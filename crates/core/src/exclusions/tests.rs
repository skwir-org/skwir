use super::*;

fn table(text: &str) -> toml::Value {
    toml::Value::Table(toml::from_str(text).unwrap())
}

fn catalog(categories: &str, entries: &str) -> Catalog {
    Catalog::from_values(
        Some(&table(categories)),
        Some(&table(entries)),
        &Legacy::default(),
    )
}

/// skwir's entry for `text` (a folder as the table writes it, `~` for home).
fn shipped<'a>(catalog: &'a Catalog, text: &str) -> &'a Entry {
    catalog
        .entries
        .iter()
        .find(|e| matches!(&e.matcher, Matcher::Pattern { text: t, beside: None, .. } if t == text))
        .unwrap_or_else(|| panic!("skwir ships {text}"))
}

fn private_marker(catalog: &Catalog) -> &Entry {
    catalog
        .entries
        .iter()
        .find(|e| e.matcher == Matcher::Holding(".private".into()))
        .expect("skwir ships the .private rule")
}

#[test]
fn skwirs_list_reads_and_each_entry_is_in_a_category() {
    let shipped = Catalog::shipped();
    assert!(shipped.problems.is_empty(), "{:?}", shipped.problems);
    let categories: HashSet<&str> = shipped.categories.iter().map(|c| c.id.as_str()).collect();
    let mut ids = HashSet::new();
    for entry in &shipped.entries {
        assert!(entry.id.starts_with(BUILTIN), "{}", entry.id);
        assert!(ids.insert(&entry.id), "{} twice", entry.id);
        assert!(categories.contains(entry.category.as_str()), "{}", entry.id);
        assert!(!entry.what.is_empty(), "{} says what it is", entry.id);
        entry
            .matcher
            .check()
            .unwrap_or_else(|e| panic!("{}: {e}", entry.id));
    }
    assert!(categories.contains(YOURS));
    // Secrets are kept from agents, not from your own searches.
    let secrets = shipped
        .category("skwir.secrets")
        .expect("a Secrets category");
    assert!(secrets.switches.get(Role::Agents) && !secrets.switches.get(Role::Searches));
    let ssh = shipped_entry(&shipped, "~/.ssh");
    assert!(shipped.keeps(ssh, Role::Agents) && !shipped.keeps(ssh, Role::Searches));
    let agents = shipped.rule(Role::Agents);
    let home = crate::os_path::home().unwrap();
    assert!(agents.names(&home.join(".ssh")));
    for marker in [
        ".private",
        "trustdb.gpg",
        ".gpg-id",
        "key4.db",
        "Login Data",
    ] {
        assert!(
            agents.markers().contains(&OsString::from(marker)),
            "{marker}"
        );
    }
    let searches = shipped.rule(Role::Searches);
    assert!(searches.names(Path::new("/anywhere/node_modules")));
    assert!(searches.names(Path::new("/anywhere/.git")));
    assert!(!searches.names(&home.join(".ssh")));
    assert!(searches.markers().contains(&OsString::from(CACHEDIR_TAG)));
}

fn shipped_entry<'a>(catalog: &'a Catalog, text: &str) -> &'a Entry {
    shipped(catalog, text)
}

#[test]
fn what_you_change_is_stored_and_nothing_more() {
    let list = catalog(
        r#"
        "skwir.secrets" = { searches = true }
        work = { name = "Work", agents = false }
        "#,
        r#"
        vm-images = { pattern = "~/VMs", what = "Virtual machines", category = "work", searches = true, agents = false, index = false }
        "#,
    );
    assert!(list.problems.is_empty(), "{:?}", list.problems);
    let ssh = shipped(&list, "~/.ssh");
    assert!(list.keeps(ssh, Role::Searches), "the category's switch");
    let vms = list.entries.iter().find(|e| e.id == "vm-images").unwrap();
    assert!(list.keeps(vms, Role::Searches) && !list.keeps(vms, Role::Agents));
    assert!(vms.changed() && !ssh.changed());
    let (categories, entries) = list.values();
    let again = Catalog::from_values(Some(&categories), Some(&entries), &Legacy::default());
    assert_eq!(again.entries, list.entries, "read back as written");
    assert_eq!(again.categories, list.categories);
    // skwir's own, untouched, are not written.
    let written = entries.as_table().unwrap();
    assert_eq!(written.keys().collect::<Vec<_>>(), ["vm-images"]);
    let none = Catalog::from_values(None, None, &Legacy::default()).values();
    assert_eq!(none.0, toml::Value::Table(toml::Table::new()));
    assert_eq!(none.1, toml::Value::Table(toml::Table::new()));
}

/// What the list could not take in is written back as it stood, however
/// the rest changes: a tick on the page never erases another system's
/// override or what a hand wrote wrong.
#[test]
fn what_the_list_cannot_hold_is_written_back_as_it_stood() {
    let (categories, entries) = (
        table(
            r#"
            "skwir.another-system" = { searches = true }
            "a.b" = { name = "x" }
            mine = { name = "Mine" }
            "#,
        ),
        table(
            r#"
            "skwir.another-system.thing" = { agents = false }
            broken = { pattern = "~/x", colour = "red" }
            waiting = { pattern = "~/y", category = "not-here" }
            "#,
        ),
    );
    let mut list = Catalog::from_values(Some(&categories), Some(&entries), &Legacy::default());
    // A tick and a new entry elsewhere.
    let ssh_id = shipped(&Catalog::shipped(), "~/.ssh").id.clone();
    let ssh = list.entries.iter_mut().find(|e| e.id == ssh_id).unwrap();
    ssh.ticks = ssh.ticks.with(Role::Searches, false);
    let (written_categories, written) = list.values();
    let written = written.as_table().unwrap();
    assert_eq!(
        written.get("skwir.another-system.thing"),
        entries.get("skwir.another-system.thing"),
    );
    assert_eq!(written.get("broken"), entries.get("broken"));
    assert_eq!(
        written["waiting"]["category"].as_str(),
        Some("not-here"),
        "the category asked for, not the one it stands in meanwhile"
    );
    assert!(written.contains_key(&ssh_id));
    let written_categories = written_categories.as_table().unwrap();
    assert!(written_categories.contains_key("skwir.another-system"));
    assert!(written_categories.contains_key("a.b"));
    assert!(written_categories.contains_key("mine"));

    // Only what the list could not hold: what was removed from it stays
    // removed.
    list.entries.retain(|e| e.id != "waiting");
    assert!(!list.values().1.as_table().unwrap().contains_key("waiting"));

    // Said; ids set aside are not given to a new one.
    assert!(
        list.problems.iter().any(|p| p.contains("broken")),
        "{:?}",
        list.problems
    );
    assert!(list.problems.iter().any(|p| p.contains("a.b")));
    assert!(
        list.problems
            .iter()
            .any(|p| p.contains("no category not-here"))
    );
    assert!(list.entry_ids().contains("broken"));
    assert!(list.category_ids().contains("a.b"));

    // The check spares what was stored as it is, as a tick on the page
    // sends it back; a file's value it judges whole.
    let stored = Some(&entries);
    let as_stored = toml::Value::Table(written.clone());
    assert!(check_entries(&as_stored, Beside::File).is_err());
    assert!(check_entries(&as_stored, Beside::Stored(stored)).is_ok());
    let mut changed = written.clone();
    changed.insert(
        "broken".to_owned(),
        table("pattern = \"~/x\"\ncolour = \"blue\"\n"),
    );
    assert!(check_entries(&toml::Value::Table(changed), Beside::Stored(stored)).is_err());
}

#[test]
fn a_built_in_switches_and_moves_but_never_changes_its_folder() {
    let ssh_id = shipped(&Catalog::shipped(), "~/.ssh").id.clone();
    let list = catalog(
        r#"mine = { name = "Mine" }"#,
        &format!(r#""{ssh_id}" = {{ searches = false, category = "mine" }}"#),
    );
    let ssh = list.entries.iter().find(|e| e.id == ssh_id).unwrap();
    assert_eq!(ssh.category, "mine");
    assert!(ssh.changed());
    assert!(list.keeps(ssh, Role::Agents), "Mine keeps from every role");
    for refused in [
        format!(r#""{ssh_id}" = {{ pattern = "~/x" }}"#),
        format!(r#""{ssh_id}" = {{ on = false }}"#),
        r#""skwir.made-up" = { pattern = "~/x" }"#.to_owned(),
    ] {
        assert!(
            check_entries(&table(&refused), Beside::File).is_err(),
            "{refused}"
        );
    }
    // An override of a built-in a later release dropped does nothing.
    let dropped = catalog("", r#""skwir.gone-since" = { searches = false }"#);
    assert!(dropped.problems.is_empty());
}

#[test]
fn your_ids_hold_no_dot_and_your_entries_say_which_folders() {
    for refused in [
        r#""my.thing" = { pattern = "~/x" }"#,
        "thing = { }",
        r#"thing = { pattern = "~/x", holding = ".y" }"#,
        r#"thing = { pattern = "rel/x" }"#,
        r#"thing = { pattern = "regex:([" }"#,
        r#"thing = { pattern = "x", beside = "a/b" }"#,
        r#"thing = { holding = ".x", beside = "y" }"#,
        r#"thing = { pattern = "x", colour = "red" }"#,
        r#"thing = { pattern = "x", searches = "yes" }"#,
    ] {
        assert!(
            check_entries(&table(refused), Beside::File).is_err(),
            "{refused}"
        );
    }
    assert!(
        check_categories(&table("work = { }"), Beside::File).is_err(),
        "a name"
    );
    assert!(check_categories(&table(r#""a.b" = { name = "x" }"#), Beside::File).is_err());
    assert!(check_categories(&table(r#""skwir.games" = { name = "x" }"#), Beside::File).is_err());
    assert!(
        check_categories(
            &table(r#""skwir.games" = { searches = false }"#),
            Beside::File
        )
        .is_ok()
    );
    let fine = r#"
        a = { pattern = "node_modules" }
        b = { pattern = "~/Projects/*/out" }
        c = { pattern = "regex:^~/VMs/.*\\.vbox$" }
        d = { pattern = "target", beside = "Cargo.toml" }
        e = { pattern = "obj", beside = "*.csproj" }
        f = { holding = ".nosearch" }
        g = { pattern = "C:/srv/cache" }
    "#;
    if cfg!(windows) {
        assert!(check_entries(&table(fine), Beside::File).is_ok());
    } else {
        assert!(
            check_entries(
                &table(&fine.replace("C:/srv/cache", "/srv/cache")),
                Beside::File
            )
            .is_ok()
        );
    }
}

#[test]
fn each_kind_of_entry_names_its_folders() {
    let home = crate::os_path::home().unwrap();
    let matchers = [
        Matcher::Pattern {
            text: "node_modules".into(),
            beside: None,
            names: Names::Folders,
        },
        Matcher::Pattern {
            text: "cmake-build-*".into(),
            beside: None,
            names: Names::Folders,
        },
        Matcher::Pattern {
            text: "~/.cargo/registry".into(),
            beside: None,
            names: Names::Folders,
        },
        Matcher::Pattern {
            text: "~/Projects/*/out".into(),
            beside: None,
            names: Names::Folders,
        },
        Matcher::Pattern {
            text: "regex:^~/VMs/[^/]+\\.vbox$".into(),
            beside: None,
            names: Names::Folders,
        },
        Matcher::Pattern {
            text: "target".into(),
            beside: Some("Cargo.toml".into()),
            names: Names::Folders,
        },
        Matcher::Holding(".nosearch".into()),
    ];
    let rule = Rule::of(&matchers);
    assert!(rule.names(&home.join("a b/node_modules")));
    assert!(rule.names(&home.join("x/cmake-build-debug")));
    assert!(rule.names(&home.join(".cargo").join("registry")));
    assert!(rule.names(&home.join("Projects").join("app's").join("out")));
    assert!(
        !rule.names(&home.join("Projects").join("a").join("b").join("out")),
        "* stays in one name"
    );
    assert!(rule.names(&home.join("VMs").join("win ü.vbox")));
    assert!(!rule.names(&home.join("VMs").join("disk.img")));
    assert!(
        !rule.names(&home.join("x/target")),
        "beside: known only by looking"
    );
    assert_eq!(rule.beside(OsStr::new("target")).count(), 1);
    assert_eq!(rule.markers(), [OsString::from(".nosearch")]);
    assert_eq!(
        rule.fingerprint(),
        Rule::of(matchers.iter().rev()).fingerprint()
    );
    assert_ne!(rule.fingerprint(), Rule::of(&matchers[1..]).fingerprint());
}

#[test]
fn a_regex_matches_the_path_written_from_home() {
    let home = crate::os_path::home().unwrap();
    let rule = Rule::of(&[Matcher::Pattern {
        text: "regex:^~/Archive/[0-9]{4}$".into(),
        beside: None,
        names: Names::Folders,
    }]);
    assert!(rule.names(&home.join("Archive").join("2024")));
    assert!(!rule.names(&home.join("Archive").join("2024").join("x")));
    assert!(!rule.names(Path::new("/elsewhere/Archive/2024")));
}

#[test]
fn a_0_3_1_files_private_folders_are_read_among_the_rules() {
    let home = crate::os_path::home().unwrap();
    // As 0.3.1 wrote them: records, ~/.aws removed, a folder of the user's.
    let listed: Vec<String> = PRIVATE_0_3_1
        .iter()
        .filter(|f| **f != "~/.aws")
        .map(|f| (*f).to_owned())
        .chain(["~/Taxes 2025".to_owned()])
        .collect();
    let legacy = Legacy::said(Some(listed), Some(false));
    let list = Catalog::from_values(None, None, &legacy);
    let aws = shipped(&list, "~/.aws");
    assert!(
        !list.keeps(aws, Role::Agents),
        "removed in 0.3.1, so it stays"
    );
    assert!(list.keeps(shipped(&list, "~/.ssh"), Role::Agents));
    assert!(
        !list.keeps(private_marker(&list), Role::Agents),
        ".private was off"
    );
    let taxes = list
        .entries
        .iter()
        .find(|e| matches!(&e.matcher, Matcher::Pattern { text, .. } if text == "~/Taxes 2025"))
        .expect("the user's folder");
    assert!(!taxes.builtin && !taxes.id.contains('.'));
    assert!(list.keeps(taxes, Role::Agents) && !list.keeps(taxes, Role::Searches));
    assert!(list.rule(Role::Agents).names(&home.join("Taxes 2025")));
    // A folder skwir lists since 0.3.1 is not taken for one removed.
    let added_since = list.kept_from(Role::Agents).filter(|e| e.builtin).count();
    assert!(added_since > PRIVATE_0_3_1.len() - 2);
    // What the new table says wins.
    let aws_id = aws.id.clone();
    let over = Catalog::from_values(
        None,
        Some(&table(&format!(r#""{aws_id}" = {{ agents = true }}"#))),
        &legacy,
    );
    assert!(over.keeps(shipped(&over, "~/.aws"), Role::Agents));
    // Written out, the list reads the same without the old keys.
    let (categories, entries) = list.values();
    let written = Catalog::from_values(Some(&categories), Some(&entries), &Legacy::default());
    assert_eq!(
        written
            .kept_from(Role::Agents)
            .map(|e| &e.matcher)
            .collect::<Vec<_>>(),
        list.kept_from(Role::Agents)
            .map(|e| &e.matcher)
            .collect::<Vec<_>>()
    );
}

#[test]
fn lowering_privacy_is_said_and_raising_it_is_not() {
    let before = Catalog::shipped();
    let ssh_id = shipped(&before, "~/.ssh").id.clone();
    let unticked = catalog("", &format!(r#""{ssh_id}" = {{ agents = false }}"#));
    assert_eq!(exposed(&before, &unticked), ["~/.ssh"]);
    let switched = catalog(r#""skwir.secrets" = { agents = false }"#, "");
    assert!(exposed(&before, &switched).len() > 3, "every secret");
    let moved = catalog(
        r#"open = { name = "Open", agents = false }"#,
        &format!(r#""{ssh_id}" = {{ category = "open" }}"#),
    );
    assert_eq!(exposed(&before, &moved), ["~/.ssh"]);
    let raised = catalog(r#""skwir.games" = { agents = true }"#, "");
    assert!(exposed(&before, &raised).is_empty());
    let narrowed_before = catalog("", r#"mine = { pattern = "~/Secret", agents = true }"#);
    let narrowed = catalog("", r#"mine = { pattern = "~/Secret/one", agents = true }"#);
    assert_eq!(exposed(&narrowed_before, &narrowed), ["~/Secret"]);
    // A copy of skwir's made the user's keeps its folder from agents: the
    // built-in unticked takes nothing from them.
    let copied = catalog(
        "",
        &format!(
            r#""{ssh_id}" = {{ agents = false }}
ssh = {{ pattern = "~/.ssh", what = "Mine", agents = true }}"#
        ),
    );
    assert!(exposed(&before, &copied).is_empty());
    // A folder two entries kept is said once.
    let twice = catalog(
        "",
        r#"a = { pattern = "~/Both", agents = true }
b = { pattern = "~/Both", agents = true }"#,
    );
    let none = catalog("", "");
    assert_eq!(exposed(&twice, &none), ["~/Both"]);
}

#[test]
fn a_folders_text_is_read_one_way_and_checked_by_the_rules() {
    for (text, written) in [
        ("node_modules", Written::Name),
        ("*.egg-info", Written::NameGlob),
        ("~/.cargo/registry", Written::Path),
        ("/srv/cache", Written::Path),
        ("~/.var/app/*/cache", Written::PathGlob),
        ("regex:^~/x/", Written::Regex),
        (" ~/it's ü ", Written::Path),
    ] {
        assert_eq!(Written::of(text), written, "{text}");
    }
    let folder = |text: &str, beside: Option<&str>| Matcher::Pattern {
        text: text.into(),
        beside: beside.map(Into::into),
        names: Names::Folders,
    };
    assert_eq!(folder(" ~/it's ü ", None).one_path(), Some("~/it's ü"));
    for many in [
        folder("target", None),
        folder("~/x/*", None),
        folder("target", Some("Cargo.toml")),
        Matcher::Holding(".private".into()),
    ] {
        assert_eq!(many.one_path(), None, "{many:?}");
    }
    assert!(folder("target", Some("Cargo.toml")).check().is_ok());
    assert!(folder("*-build", Some("*.pro")).check().is_ok());
    for wrong in [
        folder("~/x/target", Some("Cargo.toml")),
        folder("target", Some("a/Cargo.toml")),
        folder("regex:(", None),
        folder("x/[", None),
        Matcher::Holding(String::new()),
        Matcher::Holding("a/b".into()),
    ] {
        assert!(wrong.check().is_err(), "{wrong:?}");
    }
    assert_eq!(folder("~/x/*", None).looked_up(), Some("~/x/*"));
    assert_eq!(folder("~/x", None).looked_up(), Some("~/x"));
    assert_eq!(folder("target", None).looked_up(), None);
    assert_eq!(folder("regex:^~/x", None).looked_up(), None);
    // The settings' own check says the same.
    let beside_a_path = table(r#"mine = { pattern = "~/x/target", beside = "Cargo.toml" }"#);
    assert!(check_entries(&beside_a_path, Beside::File).is_err());
}

#[test]
fn an_agent_may_not_lower_privacy_but_may_raise_it() {
    let settings = Settings::in_memory();
    declare(&settings);
    let ssh_id = shipped(&Catalog::shipped(), "~/.ssh").id.clone();
    let lowering = std::collections::BTreeMap::from([(
        ENTRIES_KEY.to_owned(),
        table(&format!(r#""{ssh_id}" = {{ agents = false }}"#)),
    )]);
    let raising = std::collections::BTreeMap::from([(
        CATEGORIES_KEY.to_owned(),
        table(r#""skwir.games" = { agents = true }"#),
    )]);
    let as_agent = |changes| {
        crate::permissions::as_agent(crate::permissions::Privacy::folders([]), || {
            refuse_exposure(&settings, changes)
        })
    };
    assert!(as_agent(&lowering).is_err());
    assert!(as_agent(&raising).is_ok());
    assert!(
        refuse_exposure(&settings, &lowering).is_ok(),
        "the user may"
    );
    // The shell's variables name folders the list follows: turning them
    // off is lowering privacy, and only a change of it is.
    let shell = |on: bool| {
        std::collections::BTreeMap::from([(
            SHELL_VARIABLES_KEY.to_owned(),
            toml::Value::Boolean(on),
        )])
    };
    let (off, on) = (shell(false), shell(true));
    let refused = as_agent(&off).expect_err("an agent may not turn them off");
    assert!(refused.to_string().contains("login shell"), "{refused}");
    assert!(as_agent(&on).is_ok(), "as it stands: no change");
    assert!(refuse_exposure(&settings, &off).is_ok(), "the user may");
}

#[test]
fn an_id_is_made_from_what_it_is() {
    let taken: HashSet<String> = ["vm-images".to_owned()].into();
    assert_eq!(unique_id("VM images", &HashSet::new()), "vm-images");
    assert_eq!(unique_id("VM images", &taken), "vm-images-2");
    assert_eq!(
        unique_id("Été 2025 / photos", &HashSet::new()),
        "été-2025-photos"
    );
    assert_eq!(unique_id("...", &HashSet::new()), "folder");
}

#[test]
fn a_path_is_joined_as_the_system_writes_one() {
    let home = Path::new(if cfg!(windows) {
        "C:\\Users\\x"
    } else {
        "/home/x"
    });
    assert_eq!(
        expand("~/.cargo/registry", Some(home)),
        Some(home.join(".cargo").join("registry"))
    );
    assert_eq!(expand("~", Some(home)), Some(home.to_path_buf()));
    assert_eq!(expand("rel/x", Some(home)), None);
    if cfg!(windows) {
        assert_eq!(
            expand("C:/srv/cache", None),
            Some(PathBuf::from("C:\\srv\\cache"))
        );
    } else {
        assert_eq!(
            expand("/srv//cache/", None),
            Some(PathBuf::from("/srv/cache"))
        );
    }
}

#[test]
fn only_a_signed_tag_makes_a_cache() {
    let tmp = tempfile::tempdir().unwrap();
    let tag = tmp.path().join(CACHEDIR_TAG);
    std::fs::write(
        &tag,
        b"Signature: 8a477f597d28d172789f06886806bc55\n# a tool\n",
    )
    .unwrap();
    assert!(tags_a_cache(&tag));
    std::fs::write(&tag, b"Signature: none").unwrap();
    assert!(!tags_a_cache(&tag));
    assert!(!tags_a_cache(tmp.path()), "a folder of that name");
}

#[test]
fn key_files_are_private_wherever_they_are_and_certificates_are_not() {
    let shipped = Catalog::shipped();
    let agents = shipped.rule(Role::Agents);
    let searches = shipped.rule(Role::Searches);
    let project = crate::os_path::home().unwrap().join("work").join("app's ü");
    for secret in [
        ".env",
        ".env.local",
        "id_ed25519",
        "id_rsa",
        "server-key.pem",
        "privkey.pem",
        "me.p12",
        "signing.jks",
        "vault.kdbx",
        "putty.ppk",
        ".env.production",
        ".env.staging.local",
        "server.key",
        "terraform.tfstate",
        "client_secret_123.apps.json",
        "AuthKey_ABC123.p8",
        "wallet.dat",
        ".Rhistory",
    ] {
        assert!(
            agents.names_file(&project.join(secret)),
            "{secret} is kept from agents"
        );
        assert!(
            !searches.names_file(&project.join(secret)),
            "{secret} is found by your searches"
        );
    }
    for public in [
        "cert.pem",
        "fullchain.pem",
        "id_ed25519.pub",
        "talk.key",
        ".envrc",
        "env",
        ".env.example",
        ".env.sample",
        ".env.template",
        "cart.p8",
    ] {
        assert!(
            !agents.names_file(&project.join(public)),
            "{public} is no secret"
        );
    }
    let home = crate::os_path::home().unwrap();
    assert!(agents.names_file(&home.join(".git-credentials")));
    assert!(agents.names_file(&home.join(".cargo").join("credentials.toml")));
    assert!(!agents.names_file(&home.join(".cargo").join("config.toml")));
    assert!(
        agents.names_file(&home.join(".bash_history")),
        "a shell's history"
    );
    assert!(!agents.names_file(&home.join(".bashrc")));
    if cfg!(windows) {
        // A path glob matches the path as Windows writes it.
        let packages = home.join(r"AppData\Local\Packages");
        assert!(agents.names(&packages.join("5319275A.WhatsAppDesktop_cv1g1gvanyjgm")));
        assert!(!agents.names(&packages.join("Microsoft.WindowsCalculator_8wekyb3d8bbwe")));
        assert!(agents.names_file(&home.join(r"AppData\Roaming\GitHub CLI\hosts.yml")));
    }
    if cfg!(target_os = "linux") {
        let store = home.join(".dotnet/corefx/cryptography/x509stores");
        assert!(agents.names(&store));
        // Browsers' and mail clients' profiles: cookies, passwords, mail.
        let config = crate::xdg::config_home().unwrap();
        for profile in [
            home.join(".mozilla/firefox/abc.default-release/cookies.sqlite"),
            config.join("google-chrome/Default/Login Data"),
            config.join("google-chrome-beta/Default"),
            config.join("BraveSoftware/Brave-Browser/Default"),
            home.join(".thunderbird/xyz.default/ImapMail"),
            home.join(".var/app/org.mozilla.firefox/.mozilla"),
            home.join(".var/app/org.signal.Signal/config/Signal"),
            config.join("Signal/sql/db.sqlite"),
        ] {
            assert!(
                profile.ancestors().any(|at| agents.names(at)),
                "{} is kept from agents",
                profile.display()
            );
        }
        assert!(
            !agents.names(&config.join("google-drive")),
            "a name only like a browser's"
        );
        // Podman's logins, under the session's runtime folder when it has one.
        if let Some(runtime) = crate::xdg::runtime_dir() {
            assert!(agents.names_file(&runtime.join("containers/auth.json")));
            assert!(!agents.names(&runtime.join("containers")));
        }
        assert!(
            shipped.rule(Role::Searches).names(&home.join(".dotnet")),
            "the toolchain still left out of searches"
        );
    }
}

#[test]
fn a_path_or_a_glob_is_looked_up_a_level_at_a_time() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    for dir in [
        ".var/app/org.it's.App/cache",
        ".config/Code/User/globalStorage",
        "R/x86-library",
    ] {
        std::fs::create_dir_all(home.join(dir)).unwrap();
    }
    std::fs::write(
        home.join(".config/Code/User/globalStorage/state.vscdb"),
        b"",
    )
    .unwrap();
    std::fs::write(home.join(".bash_history"), b"").unwrap();
    let here = |text: &str| here(text, Some(home));
    assert_eq!(here("~/.var/app/*/cache"), Some(true));
    assert_eq!(here("~/.var/app/*/data"), Some(false));
    assert_eq!(
        here("~/.config/{Code,VSCodium}/User/globalStorage/state.vscdb*"),
        Some(true)
    );
    assert_eq!(here("~/.config/{Cursor,Windsurf}/User/x"), Some(false));
    assert_eq!(here("~/.*history"), Some(true));
    assert_eq!(here("~/.*_hist"), Some(false));
    assert_eq!(here("~/R/*-library"), Some(true));
    assert_eq!(here("~/gone/*"), Some(false), "a folder that is not there");
    assert_eq!(here("~/.var"), Some(true));
    assert_eq!(here("~/nothing"), Some(false));
    assert_eq!(here("~/**/cache"), None, "no walk");
    assert_eq!(here("node_modules"), None);
    assert_eq!(here("regex:^~/R/"), None);
}

#[test]
fn a_path_starts_at_a_variable_as_the_shell_writes_one() {
    let home = crate::os_path::home().unwrap();
    let folder = |text: &str| Matcher::Pattern {
        text: text.into(),
        beside: None,
        names: Names::Folders,
    };
    // Unset: what stands for it, or nothing.
    assert_eq!(
        expand("${SKWIR_TEST_TOOL_HOME:-~/.tool}/store", Some(&home)),
        Some(home.join(".tool").join("store"))
    );
    assert_eq!(expand("$SKWIR_TEST_TOOL_HOME/store", Some(&home)), None);
    assert_eq!(
        Written::of("${SKWIR_TEST_TOOL_HOME:-~/.tool}/store"),
        Written::Path
    );
    assert_eq!(Written::of("$SKWIR_TEST_TOOL_HOME"), Written::Path);
    assert_eq!(
        Written::of("${SKWIR_TEST_TOOL_HOME}/*/cache"),
        Written::PathGlob
    );
    let unset = folder("$SKWIR_TEST_TOOL_HOME/store");
    assert!(unset.check().is_ok(), "set on another computer, or later");
    assert_eq!(
        here("$SKWIR_TEST_TOOL_HOME/store", Some(&home)),
        Some(false)
    );
    assert!(unset.said().contains("is not set"), "{}", unset.said());
    for wrong in ["${SKWIR_TEST/x", "$1/x", "${}/x", "${A B}/x"] {
        assert!(folder(wrong).check().is_err(), "{wrong}");
    }
    let before = Rule::of([&unset]);
    // Learned from the shell: a list's first folder, defaults nested.
    let tmp = tempfile::tempdir().unwrap();
    let (first, second) = (tmp.path().join("it's ü"), tmp.path().join("b"));
    let list = std::env::join_paths([&first, &second]).unwrap();
    crate::environment::learn(
        BTreeSet::from([
            "SKWIR_TEST_TOOL_HOME".to_owned(),
            "SKWIR_TEST_OUTER".to_owned(),
        ]),
        [("SKWIR_TEST_TOOL_HOME".to_owned(), list)],
    );
    assert_eq!(
        expand("$SKWIR_TEST_TOOL_HOME/store", Some(&home)),
        Some(first.join("store"))
    );
    assert_eq!(
        expand(
            "${SKWIR_TEST_OUTER:-${SKWIR_TEST_TOOL_HOME:-~/x}/y}/z",
            Some(&home)
        ),
        Some(first.join("y").join("z"))
    );
    let after = Rule::of([&unset]);
    assert_ne!(
        before.fingerprint(),
        after.fingerprint(),
        "the rule follows the variable"
    );
    assert!(after.names(&first.join("store")));
    assert!(unset.said().contains("now"), "{}", unset.said());
    // A value that is no whole path is none.
    crate::environment::learn(
        BTreeSet::from(["SKWIR_TEST_TOOL_HOME".to_owned()]),
        [("SKWIR_TEST_TOOL_HOME".to_owned(), "rel/x".into())],
    );
    assert_eq!(expand("$SKWIR_TEST_TOOL_HOME/store", Some(&home)), None);
    assert_eq!(
        variables_in("${SKWIR_A:-${SKWIR_B:-~/x}}/y"),
        ["SKWIR_A", "SKWIR_B"]
    );
}

#[test]
fn skwirs_bases_follow_their_variables_and_every_entry_names_one() {
    let shipped = shipped_table();
    let bases: HashSet<&str> = shipped.base.iter().map(|b| b.name.as_str()).collect();
    for entry in &shipped.entry {
        if let Some(under) = &entry.under {
            assert!(
                bases.contains(under.as_str()),
                "{}: no base {under}",
                entry.id
            );
        }
    }
    for base in &shipped.base {
        assert!(
            !variables_in(&base.path).is_empty(),
            "{} follows a variable",
            base.name
        );
    }
    let catalog = Catalog::shipped();
    assert!(catalog.variables().contains("XDG_CONFIG_HOME"));
    // The tests' XDG folders are moved (`isolate-xdg.sh`): the entries
    // under them follow.
    let config = crate::xdg::config_home().unwrap();
    let gh = catalog
        .entries
        .iter()
        .find(|e| e.id == "skwir.creds.gh")
        .unwrap();
    assert_eq!(
        gh.matcher,
        Matcher::Pattern {
            text: tilde(&config.join("gh/hosts.yml")),
            beside: None,
            names: Names::Files,
        }
    );
}

/// What a variable holds is those characters, whatever a glob would make
/// of them: a secret in `Keys [old]` is private there, one in a path with a
/// lone brace too. Syntax is what was written.
#[test]
fn a_variables_value_is_never_read_as_a_glob() {
    if std::env::var_os("GNUPGHOME").is_some() {
        eprintln!("skipped: this environment moves GnuPG's folder itself");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let home = crate::os_path::home();
    for name in ["Keys [old]", "a{b", "x*y", "q?r", "br[]ckets"] {
        let moved = tmp.path().join(name).join("gnupg");
        crate::environment::learn(
            BTreeSet::from(["GNUPGHOME".to_owned()]),
            [("GNUPGHOME".to_owned(), moved.clone().into_os_string())],
        );
        let catalog = Catalog::shipped();
        let rule = catalog.rule(Role::Agents);
        assert!(rule.names(&moved), "{name}: its folder is named");
        assert!(
            rule.lists(&moved.join("private-keys-v1.d")),
            "{name}: under it"
        );
        let entry = catalog
            .entries
            .iter()
            .find(|e| {
                matches!(&e.matcher, Matcher::Pattern { text, .. }
                    if expand(text, home.as_deref()).as_deref() == Some(moved.as_path()))
            })
            .unwrap_or_else(|| panic!("{name}: an entry names it"));
        assert_eq!(
            entry.matcher.one_path().map(shown),
            Some(tilde(&moved)),
            "{name}: one path, said as it is"
        );
        assert!(
            !rule.names(&tmp.path().join("Keys o").join("gnupg")),
            "{name}: `[old]` is no class"
        );
    }
    assert_eq!(Written::of("~/*/cache"), Written::PathGlob);
    assert_eq!(Written::of("/srv/[[]a[]]/b"), Written::Path);
    assert_eq!(Written::of("/srv/[ab]/c"), Written::PathGlob);
    if cfg!(windows) {
        return; // `/home` and `/srv` are no whole paths there.
    }
    // What was written as a glob stays one, under a home that holds a
    // glob's characters: only what follows it is read as syntax.
    assert_eq!(
        glob_text("~/*/cache", Some(Path::new("/home/it's [x]"))).as_deref(),
        Some("/home/it's [[]x[]]/*/cache")
    );
    // Written plain by a hand, too.
    assert_eq!(
        expand("/srv/[[]a[]]/b", None),
        Some(PathBuf::from("/srv/[a]/b"))
    );
}

/// A tool that reads every file of a list (kubectl) has each of them
/// private, each its own entry; a tool's home stays the list's first.
#[test]
fn a_list_of_credentials_is_private_in_every_element() {
    // Skwir's own environment comes before the shell's
    // (`environment::var`), and a CI runner sets CARGO_HOME: the values
    // learned below are the only ones.
    // SAFETY: nextest runs each test in a process of its own, and nothing
    // in it has started a thread that reads the environment yet.
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("KUBECONFIG");
        std::env::remove_var("CARGO_HOME");
    }
    let tmp = tempfile::tempdir().unwrap();
    let (dev, prod, third) = (
        tmp.path().join("dev.yaml"),
        tmp.path().join("it's [prod].yaml"),
        tmp.path().join("third ü.yaml"),
    );
    let list = std::env::join_paths([&dev, &prod, &third]).unwrap();
    crate::environment::learn(
        BTreeSet::from(["KUBECONFIG".to_owned()]),
        [("KUBECONFIG".to_owned(), list)],
    );
    let catalog = Catalog::shipped();
    let rule = catalog.rule(Role::Agents);
    for file in [&dev, &prod, &third] {
        assert!(rule.names_file(file), "{}", file.display());
    }
    assert!(!rule.names_file(&tmp.path().join("other.yaml")));
    let ids: Vec<&str> = catalog
        .entries
        .iter()
        .filter(|e| e.id.starts_with("skwir.creds.kubeconfig"))
        .map(|e| e.id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "skwir.creds.kubeconfig",
            "skwir.creds.kubeconfig.also-2",
            "skwir.creds.kubeconfig.also-3",
        ]
    );
    // A home is where tools write: the first alone.
    let cargo = tmp.path().join("cargo a");
    let other = tmp.path().join("cargo b");
    crate::environment::learn(
        BTreeSet::from(["CARGO_HOME".to_owned()]),
        [(
            "CARGO_HOME".to_owned(),
            std::env::join_paths([&cargo, &other]).unwrap(),
        )],
    );
    let names_registry = |at: &Path| {
        Catalog::shipped()
            .rule(Role::Searches)
            .names(&at.join("registry"))
    };
    assert!(names_registry(&cargo));
    assert!(!names_registry(&other));
}

#[test]
fn a_keyring_moved_or_copied_is_told_by_what_it_holds() {
    let tmp = tempfile::tempdir().unwrap();
    let backup = tmp.path().join("old drive's backup ü");
    let gnupg = backup.join("gnupg");
    let pass = backup.join("passwords");
    std::fs::create_dir_all(gnupg.join("private-keys-v1.d")).unwrap();
    std::fs::create_dir_all(pass.join("mail")).unwrap();
    std::fs::write(gnupg.join("trustdb.gpg"), b"").unwrap();
    std::fs::write(pass.join(".gpg-id"), b"someone").unwrap();
    std::fs::write(backup.join("notes.txt"), b"").unwrap();
    let privacy = crate::permissions::Privacy::ruled(Catalog::shipped().rule(Role::Agents));
    assert!(privacy.holds(&gnupg.join("private-keys-v1.d").join("A1.key")));
    assert!(privacy.holds(&pass.join("mail").join("work.gpg")));
    assert!(
        !privacy.holds(&backup.join("notes.txt")),
        "beside them, readable"
    );
}

#[test]
fn a_secret_moved_by_its_variable_is_private_where_it_was_too() {
    if std::env::var_os("GNUPGHOME").is_some() {
        eprintln!("skipped: this environment moves GnuPG's folder itself");
        return;
    }
    let home = crate::os_path::home().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let moved = tmp.path().join("keys ü");
    let catalog = Catalog::shipped();
    assert!(
        !catalog
            .entries
            .iter()
            .any(|e| e.id.ends_with("before-it-moved") && e.id.contains("gnupg")),
        "nothing moved, nothing left behind"
    );
    crate::environment::learn(
        BTreeSet::from(["GNUPGHOME".to_owned()]),
        [("GNUPGHOME".to_owned(), moved.clone().into_os_string())],
    );
    let catalog = Catalog::shipped();
    let text_of = |id: &str| {
        catalog
            .entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.matcher.clone())
    };
    assert_eq!(
        text_of("skwir.keys.gnupg"),
        Some(Matcher::Pattern {
            text: tilde(&moved),
            beside: None,
            names: Names::Folders,
        })
    );
    assert_eq!(
        text_of("skwir.keys.gnupg.before-it-moved"),
        Some(Matcher::Pattern {
            text: "~/.gnupg".into(),
            beside: None,
            names: Names::Folders,
        })
    );
    let agents = catalog.rule(Role::Agents);
    assert!(agents.names(&moved));
    assert!(agents.names(&home.join(".gnupg")));
}

#[test]
fn a_variables_value_is_read_as_a_tool_reads_it() {
    let home = crate::os_path::home().unwrap();
    let say = |value: &str| {
        crate::environment::learn(
            BTreeSet::from(["SKWIR_TEST_LIST".to_owned()]),
            [("SKWIR_TEST_LIST".to_owned(), value.into())],
        );
        expand("${SKWIR_TEST_LIST:-~/default}/y", Some(&home))
    };
    #[cfg(unix)]
    {
        assert_eq!(say("/a:/b"), Some(PathBuf::from("/a/y")), "a list's first");
        assert_eq!(
            say(":/b"),
            Some(home.join("default").join("y")),
            "an empty first element asks for the default"
        );
    }
    assert_eq!(
        say("~/moved"),
        Some(home.join("moved").join("y")),
        "the home folder, as a tool that expands it reads it"
    );
    assert_eq!(
        say("rel/x"),
        Some(home.join("default").join("y")),
        "no whole path, no value"
    );
    // conda's list is separated by commas. A whole path as this system
    // writes one (`/a` is none on Windows).
    let (first, second) = (home.join("a"), home.join("b"));
    crate::environment::learn(
        BTreeSet::from(["SKWIR_TEST_LIST".to_owned()]),
        [(
            "SKWIR_TEST_LIST".to_owned(),
            format!("{},{}", first.display(), second.display()).into(),
        )],
    );
    assert_eq!(
        variable_path("SKWIR_TEST_LIST", Some(&home), Some(',')),
        Some(first)
    );
    assert_eq!(written_under(Path::new("/a"), ""), "/a");
    assert_eq!(written_under(Path::new("/a"), "b"), "/a/b");
}

#[test]
fn a_name_is_of_files_or_of_folders_and_they_are_told_apart() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("it's a project");
    let environment = project.join("python").join(".env");
    std::fs::create_dir_all(environment.join("lib")).unwrap();
    std::fs::write(environment.join("lib").join("site.py"), b"").unwrap();
    let keys = project.join("service");
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(keys.join(".env"), b"KEY=x").unwrap();
    let privacy = crate::permissions::Privacy::ruled(Catalog::shipped().rule(Role::Agents));
    assert!(privacy.holds(&keys.join(".env")), "a file of keys");
    assert!(
        !privacy.holds(&environment),
        "a folder of that name is a Python environment"
    );
    assert!(!privacy.holds(&environment.join("lib").join("site.py")));
    assert!(
        privacy.holds(&keys.join("new").join(".env")),
        "one not there yet cannot be told: it stays private"
    );
    let said = |text: &str, names| {
        Matcher::Pattern {
            text: text.into(),
            beside: None,
            names,
        }
        .said()
    };
    assert_eq!(
        said("*.kdbx", Names::Files),
        "any file whose name is like *.kdbx"
    );
    assert_eq!(said("target", Names::Folders), "any folder named target");
    assert_eq!(said("x", Names::Both), "any file or folder named x");
    assert_eq!(said("~/a/*.log", Names::Files), "the files like ~/a/*.log");
    // What it names is read back from the settings; a hand's says both.
    let catalog = catalog(
        "",
        r#"logs = { pattern = "*.log", names = "files" }
any = { pattern = "scratch" }"#,
    );
    assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
    let rule = catalog.rule(Role::Searches);
    assert!(rule.names_file(Path::new("/a/b.log")) && !rule.names(Path::new("/a/b.log")));
    assert!(rule.names_file(Path::new("/a/scratch")) && rule.names(Path::new("/a/scratch")));
    let wrong = table(r#"logs = { pattern = "*.log", names = "lines" }"#);
    assert!(check_entries(&wrong, Beside::File).is_err());
    let beside_files = table(r#"x = { pattern = "out", names = "files", beside = "a.toml" }"#);
    assert!(check_entries(&beside_files, Beside::File).is_err());
}
