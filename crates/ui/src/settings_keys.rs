//! The Keyboard page's reading of the keymap (ADR-024): each action, a
//! command with its arguments, once in each place its keys work, all its
//! keys as chips; what a change writes in the user's records; and what new
//! keys would do where they work. Nothing here draws: the page is drawn by
//! `settings_keys_view.rs`.

use serde_json::Value;
use skwir_core::App;
use skwir_core::keymap::UserBinding;

use crate::keymap::{self, Bound, Layer};
use crate::settings_records::{Record, new_id};

/// What the page lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Show {
    /// The actions that have keys.
    #[default]
    Keyed,
    /// Every action, those without keys too.
    All,
    /// The actions whose keys the user changed.
    Changed,
}

/// The group of the actions no key runs, listed on request.
pub(crate) const NO_KEYS: &str = "Without keys";

/// One key of an action.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Chip {
    pub keys: String,
    /// The context it is bound in, which a record's `when` says.
    pub context: String,
    /// What it adds to its place, in words; empty for nothing.
    pub conditions: String,
    pub layer: Layer,
    /// The user's record that binds it.
    pub record: Option<String>,
    /// The user's record that hides it: a default no longer run, listed
    /// struck through until restored.
    pub hidden_by: Option<String>,
}

impl Chip {
    /// Whether these keys run the action.
    pub fn runs(&self) -> bool {
        self.hidden_by.is_none()
    }
}

/// An action as the page lists it: a command with its arguments, in one
/// place.
#[derive(Clone, Debug)]
pub(crate) struct Action {
    pub place: String,
    pub rank: usize,
    pub inside: bool,
    pub command: String,
    pub args: Value,
    pub title: String,
    pub chips: Vec<Chip>,
    /// Its keys may change, never all go (`CommandSpec::keeps_a_key`).
    pub keeps_a_key: bool,
}

/// An action, known again once the keymap is read anew.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ActionId {
    pub place: String,
    pub command: String,
    pub args: String,
}

impl Action {
    pub fn id(&self) -> ActionId {
        ActionId {
            place: self.place.clone(),
            command: self.command.clone(),
            args: self.args.to_string(),
        }
    }

    /// Whether the user changed its keys: one of the user's, or a default
    /// the user's record hides.
    pub fn changed(&self) -> bool {
        self.chips
            .iter()
            .any(|chip| chip.record.is_some() || chip.hidden_by.is_some())
    }

    /// Its keys that run it.
    pub fn running(&self) -> impl Iterator<Item = &Chip> {
        self.chips.iter().filter(|chip| chip.runs())
    }

    /// Where new keys for it work: where its first key does, else
    /// anywhere.
    pub fn context(&self) -> String {
        self.running()
            .chain(self.chips.iter())
            .next()
            .map_or_else(|| keymap::ROOT.to_owned(), |chip| chip.context.clone())
    }

    /// The conditions every one of its keys adds, said once beside its
    /// title rather than on each key.
    pub fn shared_conditions(&self) -> Option<&str> {
        let first = self.chips.first()?;
        (!first.conditions.is_empty()
            && self
                .chips
                .iter()
                .all(|chip| chip.conditions == first.conditions))
        .then_some(first.conditions.as_str())
    }

    /// Whether a search finds it, by its words or by its keys
    /// (`keymap::finds`).
    fn matches(&self, query: &str) -> bool {
        let keys: Vec<&str> = self.chips.iter().map(|chip| chip.keys.as_str()).collect();
        keymap::finds(query, &[&self.title, &self.place, &self.command], &keys)
    }

    fn has_keys(&self, keys: &str) -> bool {
        self.chips.iter().any(|chip| {
            let mine = keymap::normal_keys(&chip.keys);
            mine == keys || mine.starts_with(&format!("{keys} "))
        })
    }
}

/// The keymap as the Keyboard page reads it.
#[derive(Default)]
pub(crate) struct Keyboard {
    /// In the order the page lists them: by place, then as they were
    /// bound; those without keys by title.
    pub actions: Vec<Action>,
    /// The user's bindings that wait for a plugin turned off.
    pub waiting: Vec<UserBinding>,
}

impl Keyboard {
    /// Every action of `bound` (the keymap's bindings) in its place, and
    /// every action no key runs.
    pub fn read(app: &App, bound: &[Bound], user: &[UserBinding]) -> Keyboard {
        let mut actions: Vec<Action> = Vec::new();
        let mut at: std::collections::HashMap<ActionId, usize> = std::collections::HashMap::new();
        for binding in bound {
            let Some((command, args)) = &binding.command else {
                continue;
            };
            // A binding the user's hides stays listed, struck through: a
            // default, or a shortcut of theirs written before (by hand:
            // the page never leaves one). One a scheme or the shell hides
            // is not the user's doing, and is left out as it never runs.
            let hidden_by = match binding.hidden_by.map(|i| &bound[i]) {
                None => None,
                Some(later) if later.layer == Layer::User && later.record != binding.record => {
                    later.record.clone()
                }
                Some(_) => continue,
            };
            let place = keymap::place_of(&binding.context);
            let id = ActionId {
                place: place.place.clone(),
                command: command.clone(),
                args: args.to_string(),
            };
            let index = *at.entry(id).or_insert_with(|| {
                actions.push(Action {
                    place: place.place.clone(),
                    rank: place.rank,
                    inside: place.inside,
                    command: command.clone(),
                    args: args.clone(),
                    title: keymap::title(app, command, args),
                    chips: Vec::new(),
                    keeps_a_key: false,
                });
                actions.len() - 1
            });
            let chip = Chip {
                keys: binding.keys.clone(),
                context: binding.context.clone(),
                conditions: place.conditions,
                layer: binding.layer,
                record: binding.record.clone(),
                hidden_by,
            };
            if !actions[index].chips.contains(&chip) {
                actions[index].chips.push(chip);
            }
        }
        let specs = app.commands.list();
        let kept: std::collections::HashSet<&str> = specs
            .iter()
            .filter(|spec| spec.keeps_a_key)
            .map(|spec| &*spec.name)
            .collect();
        for action in &mut actions {
            action.keeps_a_key = kept.contains(action.command.as_str());
        }
        // Stable: the places in their order, each's actions as they were
        // bound.
        actions.sort_by(|a, b| (a.inside, a.rank, &a.place).cmp(&(b.inside, b.rank, &b.place)));
        let mut unkeyed: Vec<Action> = unkeyed(app, &specs)
            .filter(|(command, args)| {
                !actions
                    .iter()
                    .any(|action| action.command == *command && action.args == *args)
            })
            .map(|(command, args)| Action {
                place: NO_KEYS.to_owned(),
                rank: usize::MAX,
                inside: false,
                title: keymap::title(app, &command, &args),
                command,
                args,
                chips: Vec::new(),
                keeps_a_key: false,
            })
            // A command called as a keyed action is (the core's twin of a
            // shell command, Toggle hidden files) is that action to whoever
            // reads the list.
            .filter(|candidate| !actions.iter().any(|action| action.title == candidate.title))
            .collect();
        // By title, each once: two commands called alike (the core's sort
        // and the shell's) are one thing to do.
        unkeyed.sort_by_cached_key(|action| action.title.to_lowercase());
        unkeyed.dedup_by(|a, b| a.title == b.title);
        let first_inside = actions
            .iter()
            .position(|action| action.inside)
            .unwrap_or(actions.len());
        actions.splice(first_inside..first_inside, unkeyed);
        Keyboard {
            actions,
            waiting: user
                .iter()
                .filter(|binding| keymap::dormant(app, binding))
                .cloned()
                .collect(),
        }
    }

    pub fn find(&self, id: &ActionId) -> Option<&Action> {
        self.actions.iter().find(|action| action.id() == *id)
    }
}

/// Every action a key could run: each form of each command, and each
/// command that needs no argument run as it is; the shell's own only as
/// the palette offers them, since the rest are keys of a place, listed
/// there. What the settings dialog edits itself is left out, as the
/// palette leaves it, and what only answers (an agent's question) does
/// nothing a key is for.
fn unkeyed<'a>(
    app: &App,
    specs: &'a [skwir_core::command::CommandSpec],
) -> impl Iterator<Item = (String, Value)> + 'a {
    let views: Vec<Value> = app
        .registry
        .views
        .all()
        .into_iter()
        .map(|view| serde_json::json!({ "id": view.id }))
        .collect();
    specs
        .iter()
        .filter(|spec| {
            !matches!(&*spec.name, "settings.apply" | "plugins.set_enabled") && !spec.data
        })
        .flat_map(|spec| {
            let shell = &*spec.owner == crate::commands::OWNER;
            let mut args: Vec<Value> = spec
                .forms
                .iter()
                .filter(|form| !shell || offered(&spec.name, &form.args))
                .map(|form| form.args.clone())
                .collect();
            if !shell && spec.required_args().is_empty() && spec.form(&Value::Null).is_none() {
                args.insert(0, serde_json::json!({}));
            }
            args.into_iter()
                .map(|args| (spec.name.to_string(), args))
                .collect::<Vec<_>>()
        })
        .chain(views.into_iter().map(|args| ("ui.view".to_owned(), args)))
}

/// Whether the palette offers the shell's command `name` with `args`: a
/// thing to do anywhere, not a key of a place.
pub(crate) fn offered(name: &str, args: &Value) -> bool {
    crate::palette::ui_commands()
        .iter()
        .any(|(n, a, _)| *n == name && a == args)
}

/// A row of the page's list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Row {
    /// A place's caption.
    Place(String),
    /// The fold of the keys inside dialogs and menus: open or not, and
    /// how many actions it holds.
    Inside { open: bool, count: usize },
    /// An action, by its index in `Keyboard::actions`.
    Action(usize),
}

/// The rows the page lists: what `show` asks for, or what matches `query`
/// (the dialog's search), every action included, or the actions bound to
/// `keys` (`keymap::normal_keys`); of those, what the page's `filter`
/// finds. The keys inside dialogs and menus are folded unless `inside` is
/// open, or a search or a filter looks for them.
pub(crate) fn rows(
    keyboard: &Keyboard,
    show: Show,
    query: &str,
    filter: &str,
    keys: Option<&str>,
    inside: bool,
) -> Vec<Row> {
    let looking =
        !query.is_empty() || !filter.is_empty() || keys.is_some() || show == Show::Changed;
    let wanted = |action: &Action| {
        let asked = match keys {
            Some(keys) => action.has_keys(keys),
            None if !query.is_empty() => action.matches(query),
            None => match show {
                Show::Keyed => !action.chips.is_empty(),
                Show::All => true,
                Show::Changed => action.changed(),
            },
        };
        asked && action.matches(filter)
    };
    let mut rows = Vec::new();
    let mut place: Option<&str> = None;
    let mut fold: Option<usize> = None;
    for (index, action) in keyboard.actions.iter().enumerate() {
        if !wanted(action) {
            continue;
        }
        if action.inside && fold.is_none() {
            fold = Some(rows.len());
            rows.push(Row::Inside {
                open: inside || looking,
                count: 0,
            });
        }
        if let Some(Row::Inside { open, count }) = fold.and_then(|at| rows.get_mut(at)) {
            *count += 1;
            if !*open {
                continue;
            }
        }
        if place != Some(action.place.as_str()) {
            place = Some(&action.place);
            rows.push(Row::Place(action.place.clone()));
        }
        rows.push(Row::Action(index));
    }
    rows
}

/// A change of the user's keys, as the page asks for it.
pub(crate) enum Edit<'a> {
    /// New keys for an action, working in `context`.
    Add {
        action: &'a Action,
        keys: String,
        context: String,
    },
    /// Other keys in place of one of its keys.
    Change {
        action: &'a Action,
        chip: &'a Chip,
        keys: String,
    },
    /// One of its keys no longer runs it.
    Remove(&'a Chip),
    /// A default the user's record hides runs again: that record goes.
    Restore(&'a Chip),
    /// Every record that touches the action goes.
    Reset(&'a Action),
}

/// A record of the user's, as the setting keeps it: `args` as JSON text,
/// left out when there are none, and no `command` to free the keys.
fn record(keys: &str, command: Option<(&str, &Value)>, context: &str) -> Record {
    let mut record = Record::new();
    record.insert("id".into(), new_id().into());
    record.insert("keys".into(), keys.into());
    if let Some((command, args)) = command {
        record.insert("command".into(), command.into());
        if args.as_object().is_some_and(|args| !args.is_empty()) {
            record.insert("args".into(), args.to_string().into());
        }
    }
    if let Some(when) = keymap::when_of(context) {
        record.insert("when".into(), when.into());
    }
    record
}

fn id_of(record: &Record) -> Option<&str> {
    record.get("id").and_then(Value::as_str)
}

/// A record's keys as GPUI reads them, and the conditions of its `when`.
fn keys_and_conditions(record: &Record) -> Option<(String, Vec<String>)> {
    let text = |key: &str| record.get(key).and_then(Value::as_str).map(str::trim);
    let keys = keymap::normal_keys(text("keys").filter(|keys| !keys.is_empty())?);
    let context = keymap::context_of(text("when").filter(|when| !when.is_empty()));
    let conditions = keymap::conditions(&context)
        .into_iter()
        .map(str::to_owned)
        .collect();
    Some((keys, conditions))
}

/// `record` after the others, so it wins where it applies (GPUI runs a
/// place's latest binding), and without the user's records of the same
/// keys whose conditions it covers: it hides them, and kept, they would
/// come back once it goes (a shortcut it replaced, a default's keys freed
/// before).
fn put(records: &mut Vec<Record>, record: Record) {
    if let Some((keys, mine)) = keys_and_conditions(&record) {
        records.retain(|other| {
            !keys_and_conditions(other).is_some_and(|(theirs_keys, theirs)| {
                theirs_keys == keys && mine.iter().all(|atom| theirs.contains(atom))
            })
        });
    }
    records.push(record);
}

/// `records` once `edit` is made: what the page saves (ADR-024). New keys
/// for an action are a record; new keys in place of a default are that
/// record and one freeing the default's keys where they worked; removing
/// a default frees its keys; removing one of the user's takes its record
/// out, changing one moves it last with its new keys. A record written
/// goes last and takes out the user's own it covers (`put`), so it is the
/// one that runs. Keys changed to the same keys change nothing.
pub(crate) fn apply(records: &mut Vec<Record>, edit: &Edit) {
    let drop_ids = |records: &mut Vec<Record>, ids: &[&str]| {
        records.retain(|record| !id_of(record).is_some_and(|id| ids.contains(&id)));
    };
    match edit {
        Edit::Add {
            action,
            keys,
            context,
        } => put(
            records,
            record(keys, Some((&action.command, &action.args)), context),
        ),
        Edit::Change { chip, keys, .. }
            if keymap::normal_keys(keys) == keymap::normal_keys(&chip.keys) => {}
        Edit::Change { action, chip, keys } => {
            if let Some(id) = &chip.record {
                if let Some(at) = records
                    .iter()
                    .position(|record| id_of(record) == Some(id.as_str()))
                {
                    let mut mine = records.remove(at);
                    mine.insert("keys".into(), keys.as_str().into());
                    put(records, mine);
                }
            } else {
                put(
                    records,
                    record(keys, Some((&action.command, &action.args)), &chip.context),
                );
                put(records, record(&chip.keys, None, &chip.context));
            }
        }
        Edit::Remove(chip) => match &chip.record {
            Some(id) => drop_ids(records, &[id.as_str()]),
            None => put(records, record(&chip.keys, None, &chip.context)),
        },
        Edit::Restore(chip) => {
            if let Some(id) = &chip.hidden_by {
                drop_ids(records, &[id.as_str()]);
            }
        }
        Edit::Reset(action) => {
            // Its own records, and those of the user's that hide its
            // defaults; not one that hides a shortcut of its own, which
            // goes with that shortcut's record.
            let ids: Vec<&str> = action
                .chips
                .iter()
                .flat_map(|chip| {
                    let hider = chip.hidden_by.as_deref().filter(|_| chip.record.is_none());
                    [chip.record.as_deref(), hider]
                })
                .flatten()
                .collect();
            drop_ids(records, &ids);
        }
    }
}

/// Where a binding runs, as `effects` compares the keymap before and after
/// a change: its action, its keys as GPUI reads them, its context, and its
/// keys as written and its command's arguments, to say what is lost.
struct Running {
    id: ActionId,
    command: String,
    args: Value,
    keys: String,
    normal: String,
    context: String,
}

/// Every binding that runs with `records` as the user's: the keymap built
/// from them as `keymap::install` builds it.
fn running(app: &App, records: &[Record]) -> Vec<Running> {
    let value = toml::Value::try_from(records).unwrap_or(toml::Value::Array(Vec::new()));
    let user = skwir_core::keymap::bindings_in(&value);
    keymap::bindings(app, &user)
        .into_iter()
        .filter(|binding| binding.hidden_by.is_none())
        .filter_map(|binding| {
            let (command, args) = binding.command?;
            Some(Running {
                id: ActionId {
                    place: keymap::place_of(&binding.context).place,
                    command: command.clone(),
                    args: args.to_string(),
                },
                command,
                args,
                normal: keymap::normal_keys(&binding.keys),
                keys: binding.keys,
                context: binding.context,
            })
        })
        .collect()
}

/// Keys another action would no longer run, where they ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Lost {
    pub title: String,
    /// Where, with its conditions in words (`context_label`).
    pub place: String,
    pub keys: String,
}

/// What a change leaves of the other actions' keys, and whether it may be
/// made at all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Effects {
    /// Keys other actions than `editing` would no longer run: those the
    /// new keys replace, and those freeing a key in a place frees beside
    /// it (Escape freed in the file list stops clearing the filter too).
    pub lost: Vec<Lost>,
    /// Why it cannot be made: an action that keeps a key would have none
    /// left in a place (`CommandSpec::keeps_a_key`).
    pub refused: Option<String>,
}

/// What making `edit` to `records` (the user's, as the dialog holds them)
/// would do to every action's keys: the keymap built before and after,
/// compared. `editing` is the action changed, whose own keys are its
/// business.
pub(crate) fn effects(
    app: &App,
    records: &[Record],
    edit: &Edit,
    editing: Option<&ActionId>,
) -> Effects {
    let mut changed = records.to_vec();
    apply(&mut changed, edit);
    if changed == records {
        return Effects::default();
    }
    let before = running(app, records);
    let after = running(app, &changed);
    let still: std::collections::HashSet<(&ActionId, &str, &str)> = after
        .iter()
        .map(|run| (&run.id, run.normal.as_str(), run.context.as_str()))
        .collect();
    let left: std::collections::HashSet<&ActionId> = after.iter().map(|run| &run.id).collect();
    let kept: std::collections::HashSet<String> = app
        .commands
        .list()
        .into_iter()
        .filter(|spec| spec.keeps_a_key)
        .map(|spec| spec.name.to_string())
        .collect();
    let mut effects = Effects::default();
    for run in &before {
        if effects.refused.is_none() && kept.contains(&run.command) && !left.contains(&run.id) {
            let title = keymap::title(app, &run.command, &run.args);
            effects.refused = Some(needs_a_key(&title, &run.id.place));
        }
        if Some(&run.id) == editing
            || still.contains(&(&run.id, run.normal.as_str(), run.context.as_str()))
        {
            continue;
        }
        let lost = Lost {
            title: keymap::title(app, &run.command, &run.args),
            place: keymap::context_label(&run.context),
            keys: run.keys.clone(),
        };
        if !effects.lost.contains(&lost) {
            effects.lost.push(lost);
        }
    }
    effects
}

/// What new keys would do where they work, said before they are saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Note {
    /// They run this there now, and would no longer.
    Replaces { title: String, place: String },
    /// They keep running this where it is bound, as `why` says.
    Stays {
        title: String,
        place: String,
        why: Stay,
    },
    /// A shortcut of the user's on the same keys waits for its plugin; it
    /// `goes` when the new keys cover where it would work.
    Waits { title: String, goes: bool },
    /// A character bound in the file list stops typing a name there.
    Typing,
    /// A key that types a character still types it in a text field: the
    /// field keeps its characters (`keymap::typed_in_fields`).
    Fields,
    /// The action has these keys there already.
    Has,
}

/// Why keys bound anew still run another action somewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stay {
    /// Bound anywhere, they meet a place that binds them itself, and GPUI
    /// takes the deepest place first.
    Deeper,
    /// Bound in a place, they leave the rest of the window as it was.
    Elsewhere,
    /// The same place, under conditions the new keys do not cover.
    Besides,
}

/// What new keys would do, and why they cannot be kept, if they cannot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub notes: Vec<Note>,
    pub refused: Option<String>,
}

/// What binding `keys` to `action` in `context`, in place of `replacing`
/// (one of its keys), would do: GPUI runs the deepest place's binding,
/// then the latest, so the user's hides those of the same keys that ask
/// for no less (`keymap::bindings`).
pub(crate) fn verdict(
    app: &App,
    keyboard: &Keyboard,
    bound: &[Bound],
    keys: &str,
    context: &str,
    action: &Action,
    replacing: Option<&Chip>,
) -> Verdict {
    let mut verdict = Verdict::default();
    let keys = keymap::normal_keys(keys);
    // The keys it has, pressed again: nothing to change.
    if replacing.is_some_and(|chip| keymap::normal_keys(&chip.keys) == keys) {
        verdict.notes.push(Note::Has);
        return verdict;
    }
    let mine = keymap::conditions(context);
    for binding in bound.iter().filter(|b| b.hidden_by.is_none()) {
        let Some((command, args)) = &binding.command else {
            continue;
        };
        let theirs_keys = keymap::normal_keys(&binding.keys);
        if theirs_keys != keys
            || replacing.is_some_and(|chip| {
                keymap::normal_keys(&chip.keys) == theirs_keys && chip.context == binding.context
            })
        {
            continue;
        }
        if *command == action.command && *args == action.args {
            if binding.context == context {
                verdict.notes.push(Note::Has);
            }
            continue;
        }
        let title = keymap::title(app, command, args);
        let place = keymap::context_label(&binding.context);
        let theirs = keymap::conditions(&binding.context);
        if mine.iter().all(|atom| theirs.contains(atom)) {
            let at = keymap::place_of(&binding.context).place;
            let last = keyboard
                .actions
                .iter()
                .find(|a| a.place == at && a.command == *command && a.args == *args)
                .is_some_and(|a| a.keeps_a_key && a.running().count() == 1);
            if last && verdict.refused.is_none() {
                verdict.refused = Some(needs_a_key(&title, &at));
            }
            verdict.notes.push(Note::Replaces { title, place });
        } else {
            // Keys of another place, neither anywhere, never meet these.
            let why = if context == keymap::ROOT {
                Stay::Deeper
            } else if binding.context == keymap::ROOT {
                Stay::Elsewhere
            } else if keymap::place_of(&binding.context).place == keymap::place_of(context).place {
                Stay::Besides
            } else {
                continue;
            };
            verdict.notes.push(Note::Stays { title, place, why });
        }
    }
    for waiting in &keyboard.waiting {
        if keymap::normal_keys(&waiting.keys) == keys
            && let Some(command) = &waiting.command
        {
            // Covered, it goes as any shortcut of the user's the new keys
            // replace (`put`).
            let theirs = keymap::context_of(waiting.when.as_deref());
            let theirs = keymap::conditions(&theirs);
            verdict.notes.push(Note::Waits {
                title: keymap::title(app, &command.name, &command.args),
                goes: mine.iter().all(|atom| theirs.contains(atom)),
            });
        }
    }
    if keymap::takes_a_character(&keys, context, true) {
        verdict.notes.push(Note::Typing);
    }
    if keymap::types_a_character(&keys) {
        verdict.notes.push(Note::Fields);
    }
    verdict
}

/// Why removing `chip` cannot be kept, if it cannot: an action that keeps
/// a key keeps its last.
pub(crate) fn removal_refused(action: &Action, chip: &Chip) -> Option<String> {
    (action.keeps_a_key && chip.runs() && action.running().count() == 1)
        .then(|| needs_a_key(&action.title, &action.place))
}

/// Why an action that keeps a key cannot lose its last in `place`.
fn needs_a_key(title: &str, place: &str) -> String {
    let title = title.trim_end_matches('…');
    if place == "Anywhere" {
        format!("{title} needs a key: give it another first")
    } else {
        format!("{title} needs a key in {place}: give it another first")
    }
}

#[cfg(test)]
mod tests {
    use super::{Edit, Keyboard, Note, Row, Show, apply, effects, removal_refused, rows, verdict};
    use crate::settings_records::Record;
    use skwir_core::App;
    use skwir_core::command::{Invocation, NoArgs, Outcome};
    use skwir_core::keymap::UserBinding;
    use skwir_core::registry::KeyBinding;

    fn app() -> (tempfile::TempDir, std::sync::Arc<App>) {
        let tmp = tempfile::tempdir().unwrap();
        let app = App::new(skwir_core::app::AppConfig::ephemeral(tmp.path())).unwrap();
        crate::commands::declare_all(&app);
        (tmp, app)
    }

    fn user(records: &[Record]) -> Vec<UserBinding> {
        let settings = skwir_core::settings::Settings::in_memory();
        skwir_core::keymap::declare(&settings, std::sync::Weak::new());
        let spec = settings.spec(skwir_core::keymap::BINDINGS_KEY).unwrap();
        let text = serde_json::to_string(records).unwrap();
        settings.set(skwir_core::keymap::BINDINGS_KEY, spec.parse(&text).unwrap());
        skwir_core::keymap::user_bindings(&settings)
    }

    fn read(app: &App, records: &[Record]) -> (Keyboard, Vec<crate::keymap::Bound>) {
        let user = user(records);
        let bound = crate::keymap::bindings(app, &user);
        (Keyboard::read(app, &bound, &user), bound)
    }

    fn action<'a>(keyboard: &'a Keyboard, place: &str, command: &str) -> &'a super::Action {
        keyboard
            .actions
            .iter()
            .find(|a| a.place == place && a.command == command)
            .unwrap_or_else(|| panic!("{command} in {place}"))
    }

    /// An action is listed once in each place, all its keys as chips.
    #[test]
    fn an_action_is_one_row_with_all_its_keys() {
        let (_tmp, app) = app();
        let (keyboard, _) = read(&app, &[]);
        let hidden = action(&keyboard, "Anywhere", "toggle_hidden");
        let keys: Vec<&str> = hidden.chips.iter().map(|c| c.keys.as_str()).collect();
        assert_eq!(keys, ["ctrl-h", "alt-."]);
        assert_eq!(hidden.title, "Toggle hidden files");
        // The settings' several contexts are one place.
        let adjust: Vec<&super::Action> = keyboard
            .actions
            .iter()
            .filter(|a| a.command == "ui.settings_adjust")
            .collect();
        assert!(adjust.iter().all(|a| a.place == "Settings"));
        assert_eq!(adjust.len(), 2, "less and more");
        // Anywhere first; the keys inside dialogs and menus last.
        assert_eq!(keyboard.actions[0].place, "Anywhere");
        let inside = keyboard.actions.iter().position(|a| a.inside).unwrap();
        assert!(keyboard.actions[inside..].iter().all(|a| a.inside));
    }

    /// The rows: places' captions, the fold of what is inside dialogs,
    /// what a search or a key finds, what the user changed.
    #[test]
    fn the_rows_are_what_is_asked_for() {
        let (_tmp, app) = app();
        let mine = [record(&[
            ("id", "1"),
            ("keys", "ctrl-j"),
            ("command", "toggle_hidden"),
        ])];
        let (keyboard, _) = read(&app, &mine);
        let folded = rows(&keyboard, Show::Keyed, "", "", None, false);
        assert_eq!(folded[0], Row::Place("Anywhere".into()));
        let Some(Row::Inside { open: false, count }) = folded.last() else {
            panic!("{:?}", folded.last());
        };
        assert!(*count > 0);
        assert!(
            !folded
                .iter()
                .any(|row| matches!(row, Row::Place(p) if p == super::NO_KEYS)),
            "nothing without keys unless asked"
        );
        let all = rows(&keyboard, Show::All, "", "", None, false);
        assert!(all.contains(&Row::Place(super::NO_KEYS.into())));
        let changed = rows(&keyboard, Show::Changed, "", "", None, false);
        let titles: Vec<&str> = changed
            .iter()
            .filter_map(|row| match row {
                Row::Action(i) => Some(keyboard.actions[*i].title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(titles, ["Toggle hidden files"]);
        let found = rows(&keyboard, Show::Keyed, "", "", Some("ctrl-j"), false);
        assert!(found.contains(&Row::Place("Anywhere".into())));
        // A search finds actions without keys too.
        let searched = rows(&keyboard, Show::Keyed, "sort by rating", "", None, false);
        assert!(
            searched
                .iter()
                .any(|row| matches!(row, Row::Action(i) if keyboard.actions[*i].chips.is_empty()))
        );
        // The page's filter narrows what it shows, by words or by keys.
        let titles = |rows: &[Row]| -> Vec<String> {
            rows.iter()
                .filter_map(|row| match row {
                    Row::Action(i) => Some(keyboard.actions[*i].title.clone()),
                    _ => None,
                })
                .collect()
        };
        let filtered = rows(&keyboard, Show::Keyed, "", "hidden files", None, false);
        assert_eq!(titles(&filtered), ["Toggle hidden files"]);
        let by_keys = rows(&keyboard, Show::Keyed, "", "ctrl+j", None, false);
        assert_eq!(titles(&by_keys), ["Toggle hidden files"]);
        let keyed = rows(&keyboard, Show::Keyed, "", "sort by rating", None, false);
        assert!(
            titles(&keyed).len() < titles(&searched).len(),
            "a filter keeps to what the page shows"
        );
    }

    fn record(fields: &[(&str, &str)]) -> Record {
        fields
            .iter()
            .map(|(k, v)| ((*k).to_owned(), serde_json::Value::from(*v)))
            .collect()
    }

    fn written(records: &[Record]) -> Vec<(String, Option<String>, Option<String>)> {
        records
            .iter()
            .map(|r| {
                let text = |key: &str| r.get(key).and_then(|v| v.as_str()).map(str::to_owned);
                (text("keys").unwrap(), text("command"), text("when"))
            })
            .collect()
    }

    /// New keys in place of a default free the default's where they
    /// worked; removing a default frees it; the user's own are edited;
    /// Reset takes every record touching the action away.
    #[test]
    fn a_change_writes_what_it_says() {
        let (_tmp, app) = app();
        let (keyboard, _) = read(&app, &[]);
        let rename = action(&keyboard, "File list", "ui.rename_prompt");
        let f2 = rename.chips.iter().find(|c| c.keys == "f2").unwrap();
        let mut records = Vec::new();
        apply(
            &mut records,
            &Edit::Change {
                action: rename,
                chip: f2,
                keys: "ctrl-r".into(),
            },
        );
        assert_eq!(
            written(&records),
            [
                (
                    "ctrl-r".into(),
                    Some("ui.rename_prompt".into()),
                    Some("pane single !restores".into())
                ),
                ("f2".into(), None, Some("pane single !restores".into())),
            ]
        );
        let (keyboard, _) = read(&app, &records);
        let rename = action(&keyboard, "File list", "ui.rename_prompt");
        assert!(rename.changed());
        let struck = rename.chips.iter().find(|c| c.keys == "f2").unwrap();
        assert!(struck.hidden_by.is_some(), "F2 is listed, struck through");
        let mine = rename.chips.iter().find(|c| c.keys == "ctrl-r").unwrap();
        let mut again = records.clone();
        apply(
            &mut again,
            &Edit::Change {
                action: rename,
                chip: mine,
                keys: "ctrl-e".into(),
            },
        );
        assert_eq!(again.len(), 2);
        assert_eq!(
            written(&again)[1].0,
            "ctrl-e",
            "the record is edited, and moved last to run"
        );
        let mut restored = records.clone();
        apply(&mut restored, &Edit::Restore(struck));
        assert_eq!(written(&restored).len(), 1, "the freeing record went");
        apply(&mut records, &Edit::Reset(rename));
        assert!(records.is_empty());

        let (keyboard, _) = read(&app, &[]);
        let hidden = action(&keyboard, "Anywhere", "toggle_hidden");
        let mut records = Vec::new();
        apply(&mut records, &Edit::Remove(&hidden.chips[0]));
        assert_eq!(written(&records), [("ctrl-h".into(), None, None)]);
        apply(
            &mut records,
            &Edit::Add {
                action: hidden,
                keys: "ctrl-j".into(),
                context: hidden.context(),
            },
        );
        assert_eq!(written(&records)[1].1.as_deref(), Some("toggle_hidden"));
    }

    /// Before saving: what the keys do now where they would work, a place
    /// that keeps its own, a shortcut waiting for its plugin, a letter in
    /// the file list; an action that keeps a key keeps its last.
    #[test]
    fn new_keys_say_what_they_would_do() {
        let (_tmp, app) = app();
        app.registry.keymap.add(
            "plugin",
            KeyBinding {
                keys: "ctrl-u".into(),
                command: Invocation::new("plugin.run"),
                when: Some("pane".into()),
            },
        );
        app.commands.register::<NoArgs, _>(
            "plugin",
            skwir_core::command::Topic::Files,
            "plugin.run",
            "Run it",
            "",
            |_, _| Ok(Outcome::Done),
        );
        let waiting = [record(&[
            ("id", "w"),
            ("keys", "ctrl-alt-w"),
            ("command", "gone.open"),
        ])];
        let (keyboard, bound) = read(&app, &waiting);
        let hidden = action(&keyboard, "Anywhere", "toggle_hidden");
        let anywhere = hidden.context();
        let say = |keys: &str, context: &str| {
            verdict(&app, &keyboard, &bound, keys, context, hidden, None)
        };
        // Ctrl+Q quits anywhere: replaced.
        assert!(matches!(
            say("ctrl-q", &anywhere).notes.as_slice(),
            [Note::Replaces { title, .. }] if title == "Quit"
        ));
        // Ctrl+U in the file list is the plugin's, which comes first there.
        assert!(matches!(
            say("ctrl-u", &anywhere).notes.as_slice(),
            [Note::Stays {
                why: super::Stay::Deeper,
                ..
            }]
        ));
        assert!(matches!(
            say("ctrl-alt-w", &anywhere).notes.as_slice(),
            [Note::Waits { title, goes: true }] if title == "Gone open"
        ));
        assert_eq!(
            say("j", &crate::keymap::context_of(Some("pane"))).notes,
            [Note::Typing, Note::Fields]
        );
        // A character bound anywhere still types in a text field.
        assert!(say(".", &anywhere).notes.contains(&Note::Fields));
        assert!(!say("ctrl-.", &anywhere).notes.contains(&Note::Fields));
        assert_eq!(say("ctrl-h", &anywhere).notes, [Note::Has]);
        // The palette keeps a key: its last cannot go, nor be taken.
        let palette = action(&keyboard, "Anywhere", "ui.palette");
        assert!(palette.keeps_a_key);
        assert!(removal_refused(palette, &palette.chips[0]).is_some());
        assert!(
            say("ctrl-shift-p", &anywhere).refused.is_some(),
            "binding its key to another"
        );
        assert!(removal_refused(hidden, &hidden.chips[0]).is_none());
    }

    fn keys_of(action: &super::Action) -> Vec<&str> {
        action.running().map(|chip| chip.keys.as_str()).collect()
    }

    /// One of the user's keys changed is moved last and frees the freeing
    /// record it covers: F2 changed to Ctrl+R, then back to F2, renames
    /// with F2 again. Changed onto another shortcut of the user's keys, it
    /// takes them, as the verdict says.
    #[test]
    fn a_key_of_one_s_own_changed_is_the_one_that_runs() {
        let (_tmp, app) = app();
        let (keyboard, _) = read(&app, &[]);
        let rename = action(&keyboard, "File list", "ui.rename_prompt");
        let f2 = rename.chips.iter().find(|c| c.keys == "f2").unwrap();
        let mut records = Vec::new();
        apply(
            &mut records,
            &Edit::Change {
                action: rename,
                chip: f2,
                keys: "ctrl-r".into(),
            },
        );
        let (keyboard, _) = read(&app, &records);
        let rename = action(&keyboard, "File list", "ui.rename_prompt");
        assert_eq!(keys_of(rename), ["ctrl-r"]);
        let mine = rename.chips.iter().find(|c| c.keys == "ctrl-r").unwrap();
        apply(
            &mut records,
            &Edit::Change {
                action: rename,
                chip: mine,
                keys: "f2".into(),
            },
        );
        assert_eq!(records.len(), 1, "the record freeing F2 went: {records:?}");
        let (keyboard, _) = read(&app, &records);
        let rename = action(&keyboard, "File list", "ui.rename_prompt");
        assert_eq!(keys_of(rename), ["f2"], "F2 renames again");

        // Ctrl+1 changed to Ctrl+2, which another of the user's has.
        let mut records = vec![
            record(&[
                ("id", "a"),
                ("keys", "ctrl-1"),
                ("command", "toggle_hidden"),
            ]),
            record(&[
                ("id", "b"),
                ("keys", "ctrl-2"),
                ("command", "ui.toggle_sidebar"),
            ]),
        ];
        let (keyboard, bound) = read(&app, &records);
        let hidden = action(&keyboard, "Anywhere", "toggle_hidden");
        let one = hidden.chips.iter().find(|c| c.keys == "ctrl-1").unwrap();
        let said = verdict(
            &app,
            &keyboard,
            &bound,
            "ctrl-2",
            &one.context,
            hidden,
            Some(one),
        );
        assert!(
            matches!(said.notes.as_slice(), [Note::Replaces { .. }]),
            "{said:?}"
        );
        apply(
            &mut records,
            &Edit::Change {
                action: hidden,
                chip: one,
                keys: "ctrl-2".into(),
            },
        );
        let (keyboard, _) = read(&app, &records);
        assert!(keys_of(action(&keyboard, "Anywhere", "toggle_hidden")).contains(&"ctrl-2"));
        assert!(!keys_of(action(&keyboard, "Anywhere", "ui.toggle_sidebar")).contains(&"ctrl-2"));
        assert_eq!(records.len(), 1, "the one it replaced went: {records:?}");
    }

    /// A shortcut the new keys replace goes from the file: taking the new
    /// keys away never brings it back.
    #[test]
    fn a_shortcut_replaced_does_not_come_back() {
        let (_tmp, app) = app();
        let (keyboard, _) = read(&app, &[]);
        let mut records = Vec::new();
        let hidden = action(&keyboard, "Anywhere", "toggle_hidden");
        apply(
            &mut records,
            &Edit::Add {
                action: hidden,
                keys: "ctrl-j".into(),
                context: hidden.context(),
            },
        );
        let sidebar = action(&keyboard, "Anywhere", "ui.toggle_sidebar");
        apply(
            &mut records,
            &Edit::Add {
                action: sidebar,
                keys: "ctrl-j".into(),
                context: sidebar.context(),
            },
        );
        assert_eq!(records.len(), 1, "{records:?}");
        let (keyboard, _) = read(&app, &records);
        let sidebar = action(&keyboard, "Anywhere", "ui.toggle_sidebar");
        let mine = sidebar.chips.iter().find(|c| c.keys == "ctrl-j").unwrap();
        apply(&mut records, &Edit::Remove(mine));
        let (keyboard, _) = read(&app, &records);
        assert!(!keys_of(action(&keyboard, "Anywhere", "toggle_hidden")).contains(&"ctrl-j"));

        // Written by hand, one hidden by a later one of the user's is
        // listed struck, to be seen and restored.
        let hand = [
            record(&[
                ("id", "a"),
                ("keys", "ctrl-j"),
                ("command", "toggle_hidden"),
            ]),
            record(&[
                ("id", "b"),
                ("keys", "ctrl-j"),
                ("command", "ui.toggle_sidebar"),
            ]),
        ];
        let (keyboard, _) = read(&app, &hand);
        let hidden = action(&keyboard, "Anywhere", "toggle_hidden");
        let struck = hidden.chips.iter().find(|c| c.keys == "ctrl-j").unwrap();
        assert_eq!(struck.hidden_by.as_deref(), Some("b"));
        assert!(hidden.changed());
    }

    /// The keys it has, pressed again for one of its keys, change nothing:
    /// the verdict says it has them, and nothing is written.
    #[test]
    fn the_keys_it_has_pressed_again_change_nothing() {
        let (_tmp, app) = app();
        let (keyboard, bound) = read(&app, &[]);
        let rename = action(&keyboard, "File list", "ui.rename_prompt");
        let f2 = rename.chips.iter().find(|c| c.keys == "f2").unwrap();
        let said = verdict(&app, &keyboard, &bound, "f2", &f2.context, rename, Some(f2));
        assert_eq!(said.notes, [Note::Has]);
        let mut records = Vec::new();
        apply(
            &mut records,
            &Edit::Change {
                action: rename,
                chip: f2,
                keys: "f2".into(),
            },
        );
        assert!(records.is_empty());
        // Spelt otherwise: `secondary-shift-p` is Ctrl+Shift+P off a Mac.
        let palette = action(&keyboard, "Anywhere", "ui.palette");
        let chip = &palette.chips[0];
        let pressed = if cfg!(target_os = "macos") {
            "cmd-shift-p"
        } else {
            "ctrl-shift-p"
        };
        let said = verdict(
            &app,
            &keyboard,
            &bound,
            pressed,
            &chip.context,
            palette,
            Some(chip),
        );
        assert_eq!(said.notes, [Note::Has]);
    }

    /// Escape freed in the file list frees it where narrower bindings of
    /// other actions had it too (clearing the filter, leaving a search):
    /// the change says which before it is saved.
    #[test]
    fn freeing_keys_in_a_place_says_what_it_frees_beside_them() {
        let (_tmp, app) = app();
        for (command, when) in [("select_none", "pane"), ("up", "pane empty search")] {
            app.registry.keymap.add(
                "plugin",
                KeyBinding {
                    keys: "escape".into(),
                    command: Invocation::new(command),
                    when: Some(when.into()),
                },
            );
            if !app.commands.contains(command) {
                app.commands.register::<NoArgs, _>(
                    "plugin",
                    skwir_core::command::Topic::Files,
                    command,
                    command,
                    "",
                    |_, _| Ok(Outcome::Done),
                );
            }
        }
        let (keyboard, _) = read(&app, &[]);
        let none = action(&keyboard, "File list", "select_none");
        let escape = none.chips.iter().find(|c| c.keys == "escape").unwrap();
        let freed = effects(&app, &[], &Edit::Remove(escape), Some(&none.id()));
        let lost: Vec<&str> = freed.lost.iter().map(|lost| lost.title.as_str()).collect();
        let filter = crate::keymap::title(&app, "filter", &serde_json::json!({}));
        let up = crate::keymap::title(&app, "up", &serde_json::json!({}));
        assert!(lost.contains(&filter.as_str()), "{lost:?}");
        assert!(lost.contains(&up.as_str()), "{lost:?}");
        assert!(freed.refused.is_none());
        // Changed to other keys, the same: the old ones are freed.
        let changed = effects(
            &app,
            &[],
            &Edit::Change {
                action: none,
                chip: escape,
                keys: "ctrl-d".into(),
            },
            Some(&none.id()),
        );
        assert!(changed.lost.iter().any(|lost| lost.title == filter));
    }

    /// Restoring a default, or going back to skwir's keys, never takes the
    /// last key of an action that keeps one: the palette moved to F9
    /// keeps it when Toggle sidebar asks for F9 back.
    #[test]
    fn restore_and_reset_never_take_the_last_key_of_one_that_keeps_one() {
        let (_tmp, app) = app();
        let (keyboard, _) = read(&app, &[]);
        let palette = action(&keyboard, "Anywhere", "ui.palette");
        let mut records = Vec::new();
        apply(
            &mut records,
            &Edit::Change {
                action: palette,
                chip: &palette.chips[0],
                keys: "f9".into(),
            },
        );
        let (keyboard, _) = read(&app, &records);
        let sidebar = action(&keyboard, "Anywhere", "ui.toggle_sidebar");
        let struck = sidebar.chips.iter().find(|c| c.keys == "f9").unwrap();
        assert!(!struck.runs());
        let restored = effects(&app, &records, &Edit::Restore(struck), Some(&sidebar.id()));
        assert!(
            restored
                .refused
                .as_deref()
                .is_some_and(|why| why.contains("needs a key")),
            "{restored:?}"
        );
        let reset = effects(&app, &records, &Edit::Reset(sidebar), Some(&sidebar.id()));
        assert!(reset.refused.is_some(), "{reset:?}");
        // The palette given back its own key first, Toggle sidebar may
        // have F9 again.
        let palette = action(&keyboard, "Anywhere", "ui.palette");
        let back = effects(&app, &records, &Edit::Reset(palette), Some(&palette.id()));
        assert!(back.refused.is_none(), "{back:?}");
    }
}
