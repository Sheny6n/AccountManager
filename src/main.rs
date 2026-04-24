mod crypto;
mod db;
mod model;

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, radio, row, scrollable, text,
    text_input, vertical_space,
};
use iced::widget::text_input::Id as TextInputId;
use iced::{Alignment, Color, Element, Length, Subscription, Task, Theme};
use zeroize::Zeroize;

use crypto::SALT_LEN;
use db::Db;
use model::{Account, Field, Group};

fn main() -> iced::Result {
    iced::application("Account Manager", App::update, App::view)
        .theme(|_| app_theme())
        .subscription(App::subscription)
        .window_size((1080.0, 720.0))
        .run_with(App::new)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoLockTimeout {
    Never,
    OneMin,
    FiveMin,
    FifteenMin,
    ThirtyMin,
    OneHour,
}

impl AutoLockTimeout {
    const ALL: &'static [AutoLockTimeout] = &[
        AutoLockTimeout::Never,
        AutoLockTimeout::OneMin,
        AutoLockTimeout::FiveMin,
        AutoLockTimeout::FifteenMin,
        AutoLockTimeout::ThirtyMin,
        AutoLockTimeout::OneHour,
    ];

    fn seconds(self) -> Option<u64> {
        match self {
            AutoLockTimeout::Never => None,
            AutoLockTimeout::OneMin => Some(60),
            AutoLockTimeout::FiveMin => Some(300),
            AutoLockTimeout::FifteenMin => Some(900),
            AutoLockTimeout::ThirtyMin => Some(1800),
            AutoLockTimeout::OneHour => Some(3600),
        }
    }

    fn from_seconds(s: Option<u64>) -> Self {
        match s {
            None => AutoLockTimeout::Never,
            Some(n) => AutoLockTimeout::ALL
                .iter()
                .copied()
                .find(|t| t.seconds() == Some(n))
                .unwrap_or(AutoLockTimeout::Never),
        }
    }

    fn label(self) -> &'static str {
        match self {
            AutoLockTimeout::Never => "Never",
            AutoLockTimeout::OneMin => "1 minute",
            AutoLockTimeout::FiveMin => "5 minutes",
            AutoLockTimeout::FifteenMin => "15 minutes",
            AutoLockTimeout::ThirtyMin => "30 minutes",
            AutoLockTimeout::OneHour => "1 hour",
        }
    }

    fn encode(self) -> String {
        match self.seconds() {
            None => "never".to_string(),
            Some(n) => n.to_string(),
        }
    }

    fn decode(s: &str) -> Self {
        let s = s.trim();
        if s.eq_ignore_ascii_case("never") || s.is_empty() {
            AutoLockTimeout::Never
        } else {
            match s.parse::<u64>() {
                Ok(n) => AutoLockTimeout::from_seconds(Some(n)),
                Err(_) => AutoLockTimeout::Never,
            }
        }
    }
}

fn app_theme() -> Theme {
    Theme::custom(
        "AM Black".to_string(),
        iced::theme::Palette {
            background: Color::from_rgb8(8, 8, 10),
            text: Color::from_rgb8(240, 240, 245),
            primary: Color::from_rgb8(80, 180, 255),
            success: Color::from_rgb8(80, 210, 130),
            danger: Color::from_rgb8(255, 80, 100),
        },
    )
}

struct App {
    tabs: Vec<Screen>,
    active_tab: usize,
    startup_error: Option<String>,
}

enum Screen {
    Start,
    CreateProfile(CreateProfileState),
    Unlock(UnlockState),
    Main(MainState),
}

struct CreateProfileState {
    db_path: PathBuf,
    password: String,
    confirm: String,
    error: Option<String>,
}

struct UnlockState {
    db_path: PathBuf,
    password: String,
    error: Option<String>,
}

struct MainState {
    db_path: PathBuf,
    db: Db,
    salt: Option<[u8; SALT_LEN]>,
    groups: Vec<Group>,
    selected_group: Option<i64>,
    accounts: Vec<Account>,
    new_group_name: String,
    search: String,
    editor: Option<AccountEditor>,
    error: Option<String>,
    renaming_group: Option<(i64, String)>,
    group_menu_open: Option<i64>,
    settings: Option<SettingsState>,
    site_width: f32,
    field_widths: HashMap<String, f32>,
    quick_add: Vec<String>,
    auto_lock: AutoLockTimeout,
    last_activity: Instant,
}

const DEFAULT_SITE_WIDTH: f32 = 200.0;
const DEFAULT_FIELD_WIDTH: f32 = 180.0;
const ACTIONS_WIDTH: f32 = 180.0;
const COLUMN_STEP: f32 = 20.0;
const MIN_COLUMN_WIDTH: f32 = 80.0;
const MAX_COLUMN_WIDTH: f32 = 800.0;

const PREF_COL_SITE: &str = "col.site";
const PREF_COL_FIELD_PREFIX: &str = "col.field.";
const PREF_QUICK_ADD: &str = "quick_add";
const PREF_AUTO_LOCK: &str = "auto_lock_seconds";

const DEFAULT_QUICK_ADD: &[&str] = &[
    "email",
    "username",
    "region",
    "phone",
    "id",
    "notes",
];

fn encode_quick_add(list: &[String]) -> String {
    list.join("\n")
}

fn decode_quick_add(s: &str) -> Vec<String> {
    s.split('\n')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

#[derive(Debug, Clone)]
enum ColumnId {
    Site,
    Field(String),
}

#[derive(Debug, Clone, Copy)]
enum FocusFrom {
    Site,
    Key(usize),
    Value(usize),
}

fn site_input_id() -> TextInputId {
    TextInputId::new("edit-site")
}

fn key_input_id(i: usize) -> TextInputId {
    TextInputId::new(format!("edit-key-{i}"))
}

fn value_input_id(i: usize) -> TextInputId {
    TextInputId::new(format!("edit-val-{i}"))
}

#[derive(Default)]
struct SettingsState {
    new_password: String,
    confirm: String,
    error: Option<String>,
    success: Option<String>,
    quick_add_input: String,
}

#[derive(Default)]
struct AccountEditor {
    id: i64,
    site: String,
    fields: Vec<Field>,
}

#[derive(Debug, Clone)]
enum Message {
    NewTab,
    SelectTab(usize),
    CloseTab(usize),

    PickOpenPath,
    PickNewPath,

    CreatePasswordChanged(String),
    CreateConfirmChanged(String),
    CreateSubmit,
    CreateCancel,

    UnlockPasswordChanged(String),
    UnlockSubmit,
    UnlockCancel,

    LockProfile,
    SelectGroup(i64),
    NewGroupNameChanged(String),
    AddGroup,
    DeleteGroup(i64),
    StartRenameGroup(i64),
    RenameGroupChanged(String),
    ConfirmRenameGroup,
    CancelRenameGroup,
    ToggleGroupMenu(i64),
    SearchChanged(String),

    NewAccount,
    EditAccount(i64),
    DeleteAccount(i64),
    TogglePin(i64),

    ResizeColumn(ColumnId, i32),

    EditSite(String),
    EditFieldKey(usize, String),
    EditFieldValue(usize, String),
    AddField,
    AddFieldWithKey(String),
    RemoveField(usize),
    EditSave,
    EditCancel,
    EditFocusNext(FocusFrom),

    OpenSettings,
    CloseSettings,
    AutoLockChanged(AutoLockTimeout),
    SettingsNewPasswordChanged(String),
    SettingsConfirmPasswordChanged(String),
    ChangePasswordSubmit,
    QuickAddInputChanged(String),
    AddQuickAddPreset,
    RemoveQuickAddPreset(usize),
    ResetQuickAddDefaults,

    Tick,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                tabs: vec![Screen::Start],
                active_tab: 0,
                startup_error: None,
            },
            Task::none(),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        let any_timeout = self.tabs.iter().any(|s| {
            matches!(s, Screen::Main(st) if st.auto_lock.seconds().is_some())
        });
        if any_timeout {
            iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
        } else {
            Subscription::none()
        }
    }

    fn active_mut(&mut self) -> &mut Screen {
        &mut self.tabs[self.active_tab]
    }

    fn path_already_open(&self, path: &Path, skip_tab: Option<usize>) -> bool {
        self.tabs.iter().enumerate().any(|(i, s)| {
            if Some(i) == skip_tab {
                return false;
            }
            match s {
                Screen::Main(st) => st.db_path == path,
                Screen::Unlock(st) => st.db_path == path,
                Screen::CreateProfile(st) => st.db_path == path,
                Screen::Start => false,
            }
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if !matches!(message, Message::Tick) {
            if let Some(Screen::Main(st)) = self.tabs.get_mut(self.active_tab) {
                st.last_activity = Instant::now();
            }
        }
        match message {
            Message::NewTab => {
                self.tabs.push(Screen::Start);
                self.active_tab = self.tabs.len() - 1;
                self.startup_error = None;
            }
            Message::SelectTab(i) => {
                if i < self.tabs.len() {
                    self.active_tab = i;
                }
            }
            Message::CloseTab(i) => {
                if i >= self.tabs.len() {
                    return Task::none();
                }
                if self.tabs.len() == 1 {
                    self.tabs[0] = Screen::Start;
                    self.active_tab = 0;
                } else {
                    self.tabs.remove(i);
                    if self.active_tab >= self.tabs.len() {
                        self.active_tab = self.tabs.len() - 1;
                    } else if self.active_tab > i {
                        self.active_tab -= 1;
                    }
                }
            }

            Message::PickOpenPath => {
                if let Some(path) = pick_open_path() {
                    self.startup_error = None;
                    if self.path_already_open(&path, Some(self.active_tab)) {
                        self.startup_error =
                            Some("That profile is already open in another tab.".into());
                        return Task::none();
                    }
                    if is_encrypted(&path) {
                        *self.active_mut() = Screen::Unlock(UnlockState {
                            db_path: path,
                            password: String::new(),
                            error: None,
                        });
                    } else {
                        match open_profile(&path, "") {
                            Ok((db, salt)) => {
                                *self.active_mut() = Screen::Main(enter_main(path, db, salt))
                            }
                            Err(e) => {
                                self.startup_error = Some(format!("Open failed: {e}"));
                            }
                        }
                    }
                }
            }
            Message::PickNewPath => {
                if let Some(path) = pick_new_path() {
                    self.startup_error = None;
                    if self.path_already_open(&path, Some(self.active_tab)) {
                        self.startup_error =
                            Some("That profile is already open in another tab.".into());
                        return Task::none();
                    }
                    *self.active_mut() = Screen::CreateProfile(CreateProfileState {
                        db_path: path,
                        password: String::new(),
                        confirm: String::new(),
                        error: None,
                    });
                }
            }

            Message::CreatePasswordChanged(s) => {
                if let Screen::CreateProfile(st) = self.active_mut() {
                    st.password = s;
                }
            }
            Message::CreateConfirmChanged(s) => {
                if let Screen::CreateProfile(st) = self.active_mut() {
                    st.confirm = s;
                }
            }
            Message::CreateSubmit => {
                if let Screen::CreateProfile(st) = self.active_mut() {
                    if !st.password.is_empty() && st.password != st.confirm {
                        st.error = Some("Passwords don't match".into());
                        return Task::none();
                    }
                    let path = st.db_path.clone();
                    let result = create_profile(&path, &st.password)
                        .and_then(|()| open_profile(&path, &st.password));
                    st.password.zeroize();
                    st.confirm.zeroize();
                    match result {
                        Ok((db, salt)) => {
                            *self.active_mut() = Screen::Main(enter_main(path, db, salt))
                        }
                        Err(e) => st.error = Some(e),
                    }
                }
            }
            Message::CreateCancel => *self.active_mut() =Screen::Start,

            Message::UnlockPasswordChanged(s) => {
                if let Screen::Unlock(st) = self.active_mut() {
                    st.password = s;
                }
            }
            Message::UnlockSubmit => {
                if let Screen::Unlock(st) = self.active_mut() {
                    match open_profile(&st.db_path, &st.password) {
                        Ok((db, salt)) => {
                            st.password.zeroize();
                            let path = std::mem::take(&mut st.db_path);
                            *self.active_mut() = Screen::Main(enter_main(path, db, salt));
                        }
                        Err(e) => st.error = Some(e),
                    }
                }
            }
            Message::UnlockCancel => *self.active_mut() =Screen::Start,

            Message::LockProfile => *self.active_mut() =Screen::Start,
            Message::SelectGroup(id) => {
                if let Screen::Main(st) = self.active_mut() {
                    st.selected_group = Some(id);
                    st.accounts = st.db.list_accounts(id).unwrap_or_default();
                    st.editor = None;
                    st.group_menu_open = None;
                    st.search.clear();
                }
            }
            Message::SearchChanged(s) => {
                if let Screen::Main(st) = self.active_mut() {
                    st.search = s;
                }
            }
            Message::NewGroupNameChanged(s) => {
                if let Screen::Main(st) = self.active_mut() {
                    st.new_group_name = s;
                }
            }
            Message::AddGroup => {
                if let Screen::Main(st) = self.active_mut() {
                    let name = st.new_group_name.trim().to_string();
                    if !name.is_empty() && st.db.add_group(&name).is_ok() {
                        st.groups = st.db.list_groups().unwrap_or_default();
                        st.new_group_name.clear();
                        if st.selected_group.is_none() {
                            st.selected_group = st.groups.first().map(|g| g.id);
                        }
                    }
                }
            }
            Message::DeleteGroup(id) => {
                if let Screen::Main(st) = self.active_mut() {
                    let _ = st.db.delete_group(id);
                    st.groups = st.db.list_groups().unwrap_or_default();
                    if st.selected_group == Some(id) {
                        st.selected_group = st.groups.first().map(|g| g.id);
                    }
                    st.accounts = match st.selected_group {
                        Some(gid) => st.db.list_accounts(gid).unwrap_or_default(),
                        None => vec![],
                    };
                    if matches!(&st.renaming_group, Some((rid, _)) if *rid == id) {
                        st.renaming_group = None;
                    }
                    st.group_menu_open = None;
                }
            }
            Message::StartRenameGroup(id) => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some(g) = st.groups.iter().find(|g| g.id == id) {
                        st.renaming_group = Some((id, g.name.clone()));
                        st.group_menu_open = None;
                        st.error = None;
                    }
                }
            }
            Message::RenameGroupChanged(s) => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some((_, name)) = st.renaming_group.as_mut() {
                        *name = s;
                    }
                }
            }
            Message::ConfirmRenameGroup => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some((id, name)) = st.renaming_group.clone() {
                        let trimmed = name.trim().to_string();
                        if trimmed.is_empty() {
                            st.error = Some("Group name is required".into());
                            return Task::none();
                        }
                        match st.db.rename_group(id, &trimmed) {
                            Ok(()) => {
                                st.groups = st.db.list_groups().unwrap_or_default();
                                st.renaming_group = None;
                                st.error = None;
                            }
                            Err(err) => st.error = Some(format!("Rename failed: {err}")),
                        }
                    }
                }
            }
            Message::CancelRenameGroup => {
                if let Screen::Main(st) = self.active_mut() {
                    st.renaming_group = None;
                    st.error = None;
                }
            }
            Message::ToggleGroupMenu(id) => {
                if let Screen::Main(st) = self.active_mut() {
                    st.group_menu_open = if st.group_menu_open == Some(id) {
                        None
                    } else {
                        Some(id)
                    };
                }
            }

            Message::NewAccount => {
                if let Screen::Main(st) = self.active_mut() {
                    if st.selected_group.is_some() {
                        st.editor = Some(AccountEditor {
                            fields: vec![Field::default()],
                            ..Default::default()
                        });
                    }
                }
            }
            Message::EditAccount(id) => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some(a) = st.accounts.iter().find(|a| a.id == id) {
                        st.editor = Some(AccountEditor {
                            id: a.id,
                            site: a.site.clone(),
                            fields: a.fields.clone(),
                        });
                    }
                }
            }
            Message::DeleteAccount(id) => {
                if let Screen::Main(st) = self.active_mut() {
                    let _ = st.db.delete_account(id);
                    if let Some(gid) = st.selected_group {
                        st.accounts = st.db.list_accounts(gid).unwrap_or_default();
                    }
                }
            }
            Message::TogglePin(id) => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some(a) = st.accounts.iter().find(|a| a.id == id) {
                        let new_state = !a.pinned;
                        let _ = st.db.set_pinned(id, new_state);
                        if let Some(gid) = st.selected_group {
                            st.accounts = st.db.list_accounts(gid).unwrap_or_default();
                        }
                    }
                }
            }
            Message::ResizeColumn(col, delta) => {
                if let Screen::Main(st) = self.active_mut() {
                    let d = delta as f32;
                    match col {
                        ColumnId::Site => {
                            st.site_width = (st.site_width + d)
                                .clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);
                            let _ = st.db.set_pref(PREF_COL_SITE, &st.site_width.to_string());
                        }
                        ColumnId::Field(k) => {
                            let cur = *st
                                .field_widths
                                .get(&k)
                                .unwrap_or(&DEFAULT_FIELD_WIDTH);
                            let new_w = (cur + d).clamp(MIN_COLUMN_WIDTH, MAX_COLUMN_WIDTH);
                            st.field_widths.insert(k.clone(), new_w);
                            let pref_key = format!("{PREF_COL_FIELD_PREFIX}{k}");
                            let _ = st.db.set_pref(&pref_key, &new_w.to_string());
                        }
                    }
                }
            }

            Message::EditSite(s) => edit_editor(self.active_mut(), |e| e.site = s),
            Message::EditFieldKey(idx, s) => edit_editor(self.active_mut(), |e| {
                if let Some(f) = e.fields.get_mut(idx) {
                    f.key = s;
                }
            }),
            Message::EditFieldValue(idx, s) => edit_editor(self.active_mut(), |e| {
                if let Some(f) = e.fields.get_mut(idx) {
                    f.value = s;
                }
            }),
            Message::AddField => edit_editor(self.active_mut(), |e| {
                e.fields.push(Field::default());
            }),
            Message::AddFieldWithKey(k) => edit_editor(self.active_mut(), |e| {
                e.fields.push(Field {
                    key: k,
                    value: String::new(),
                });
            }),
            Message::RemoveField(idx) => edit_editor(self.active_mut(), |e| {
                if idx < e.fields.len() {
                    e.fields.remove(idx);
                }
            }),
            Message::EditSave => {
                if let Screen::Main(st) = self.active_mut() {
                    if let (Some(gid), Some(e)) = (st.selected_group, &st.editor) {
                        if e.site.trim().is_empty() {
                            st.error = Some("Site is required".into());
                            return Task::none();
                        }
                        let prev_pinned = st
                            .accounts
                            .iter()
                            .find(|x| x.id == e.id)
                            .map(|x| x.pinned)
                            .unwrap_or(false);
                        let a = Account {
                            id: e.id,
                            group_id: gid,
                            site: e.site.clone(),
                            pinned: prev_pinned,
                            fields: e.fields.clone(),
                        };
                        match st.db.upsert_account(&a) {
                            Ok(_) => match st.db.list_accounts(gid) {
                                Ok(list) => {
                                    st.accounts = list;
                                    st.editor = None;
                                    st.error = None;
                                }
                                Err(err) => st.error = Some(format!("Reload failed: {err}")),
                            },
                            Err(err) => st.error = Some(format!("Save failed: {err}")),
                        }
                    }
                }
            }
            Message::EditCancel => {
                if let Screen::Main(st) = self.active_mut() {
                    st.editor = None;
                    st.error = None;
                }
            }
            Message::EditFocusNext(from) => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some(e) = st.editor.as_ref() {
                        let next_id = match from {
                            FocusFrom::Site => {
                                if e.fields.is_empty() {
                                    None
                                } else {
                                    Some(key_input_id(0))
                                }
                            }
                            FocusFrom::Key(i) => Some(value_input_id(i)),
                            FocusFrom::Value(i) => {
                                if i + 1 < e.fields.len() {
                                    Some(key_input_id(i + 1))
                                } else {
                                    None
                                }
                            }
                        };
                        return match next_id {
                            Some(id) => text_input::focus(id),
                            None => Task::done(Message::EditSave),
                        };
                    }
                }
            }

            Message::OpenSettings => {
                if let Screen::Main(st) = self.active_mut() {
                    st.settings = Some(SettingsState::default());
                    st.editor = None;
                    st.error = None;
                }
            }
            Message::CloseSettings => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some(ss) = st.settings.as_mut() {
                        ss.new_password.zeroize();
                        ss.confirm.zeroize();
                    }
                    st.settings = None;
                }
            }
            Message::AutoLockChanged(t) => {
                if let Screen::Main(st) = self.active_mut() {
                    st.auto_lock = t;
                    st.last_activity = Instant::now();
                    let _ = st.db.set_pref(PREF_AUTO_LOCK, &t.encode());
                }
            }
            Message::SettingsNewPasswordChanged(s) => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some(ss) = st.settings.as_mut() {
                        ss.new_password = s;
                        ss.success = None;
                    }
                }
            }
            Message::SettingsConfirmPasswordChanged(s) => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some(ss) = st.settings.as_mut() {
                        ss.confirm = s;
                        ss.success = None;
                    }
                }
            }
            Message::QuickAddInputChanged(s) => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some(ss) = st.settings.as_mut() {
                        ss.quick_add_input = s;
                    }
                }
            }
            Message::AddQuickAddPreset => {
                if let Screen::Main(st) = self.active_mut() {
                    let trimmed = st
                        .settings
                        .as_ref()
                        .map(|ss| ss.quick_add_input.trim().to_string())
                        .unwrap_or_default();
                    if trimmed.is_empty() {
                        return Task::none();
                    }
                    if !st.quick_add.iter().any(|p| p == &trimmed) {
                        st.quick_add.push(trimmed);
                        let _ = st
                            .db
                            .set_pref(PREF_QUICK_ADD, &encode_quick_add(&st.quick_add));
                    }
                    if let Some(ss) = st.settings.as_mut() {
                        ss.quick_add_input.clear();
                    }
                }
            }
            Message::RemoveQuickAddPreset(idx) => {
                if let Screen::Main(st) = self.active_mut() {
                    if idx < st.quick_add.len() {
                        st.quick_add.remove(idx);
                        let _ = st
                            .db
                            .set_pref(PREF_QUICK_ADD, &encode_quick_add(&st.quick_add));
                    }
                }
            }
            Message::ResetQuickAddDefaults => {
                if let Screen::Main(st) = self.active_mut() {
                    st.quick_add = DEFAULT_QUICK_ADD.iter().map(|s| s.to_string()).collect();
                    let _ = st
                        .db
                        .set_pref(PREF_QUICK_ADD, &encode_quick_add(&st.quick_add));
                }
            }

            Message::ChangePasswordSubmit => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some(ss) = st.settings.as_mut() {
                        if ss.new_password.is_empty() {
                            ss.error = Some("Password cannot be empty".into());
                            ss.success = None;
                            return Task::none();
                        }
                        if ss.new_password != ss.confirm {
                            ss.error = Some("Passwords don't match".into());
                            ss.success = None;
                            return Task::none();
                        }
                        let salt = match st.salt {
                            Some(s) => s,
                            None => {
                                ss.error = Some("Profile is unencrypted".into());
                                ss.success = None;
                                return Task::none();
                            }
                        };
                        let mut key = match crypto::derive_key(&ss.new_password, &salt) {
                            Ok(k) => k,
                            Err(e) => {
                                ss.error = Some(format!("Derive failed: {e}"));
                                ss.success = None;
                                return Task::none();
                            }
                        };
                        let result = st.db.rekey(&key);
                        key.zeroize();
                        ss.new_password.zeroize();
                        ss.confirm.zeroize();
                        match result {
                            Ok(()) => {
                                ss.success = Some("Password changed.".into());
                                ss.error = None;
                            }
                            Err(e) => {
                                ss.error = Some(format!("Rekey failed: {e}"));
                                ss.success = None;
                            }
                        }
                    }
                }
            }

            Message::Tick => {
                for s in &mut self.tabs {
                    if let Screen::Main(st) = s {
                        if let Some(secs) = st.auto_lock.seconds() {
                            if st.last_activity.elapsed() >= Duration::from_secs(secs) {
                                *s = Screen::Start;
                            }
                        }
                    }
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let body = match &self.tabs[self.active_tab] {
            Screen::Start => start_view(self.startup_error.as_deref()),
            Screen::CreateProfile(st) => create_profile_view(st),
            Screen::Unlock(st) => unlock_view(st),
            Screen::Main(st) => main_view(st),
        };
        column![tab_bar(&self.tabs, self.active_tab), body].into()
    }
}

fn tab_label(s: &Screen) -> String {
    match s {
        Screen::Start => "New Tab".to_string(),
        Screen::CreateProfile(st) => format!("New · {}", display_name(&st.db_path)),
        Screen::Unlock(st) => format!("Unlock · {}", display_name(&st.db_path)),
        Screen::Main(st) => display_name(&st.db_path),
    }
}

fn tab_bar<'a>(tabs: &'a [Screen], active: usize) -> Element<'a, Message> {
    let mut bar = row![].spacing(4).align_y(Alignment::Center);
    for (i, s) in tabs.iter().enumerate() {
        let is_active = i == active;
        let mut label_btn = button(text(tab_label(s)).size(12))
            .padding([6, 12])
            .on_press(Message::SelectTab(i));
        label_btn = if is_active {
            label_btn.style(button::primary)
        } else {
            label_btn.style(button::secondary)
        };
        bar = bar.push(
            row![
                label_btn,
                button(text("×").size(12))
                    .padding([4, 8])
                    .on_press(Message::CloseTab(i))
                    .style(button::secondary),
            ]
            .spacing(2)
            .align_y(Alignment::Center),
        );
    }
    bar = bar.push(
        button(text("+ New Tab").size(12))
            .padding([6, 12])
            .on_press(Message::NewTab)
            .style(button::secondary),
    );
    container(scrollable(bar).direction(scrollable::Direction::Horizontal(
        scrollable::Scrollbar::default(),
    )))
    .padding([6, 10])
    .width(Length::Fill)
    .into()
}

fn edit_editor(screen: &mut Screen, f: impl FnOnce(&mut AccountEditor)) {
    if let Screen::Main(st) = screen {
        if let Some(e) = st.editor.as_mut() {
            f(e);
        }
    }
}

// ---------- file dialogs ----------

fn pick_open_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Account Manager Profile", &["am"])
        .pick_file()
}

fn pick_new_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Account Manager Profile", &["am"])
        .set_file_name("profile.am")
        .save_file()
        .map(|p| {
            if p.extension().and_then(|s| s.to_str()) == Some("am") {
                p
            } else {
                p.with_extension("am")
            }
        })
}

// ---------- profile I/O ----------

// Plain SQLite databases begin with this exact 16-byte magic string.
// SQLCipher-encrypted databases do not (the header is encrypted; the first
// 16 bytes are the random salt).
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

fn read_file_head(path: &Path, n: usize) -> Result<Vec<u8>, std::io::Error> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; n];
    let mut read = 0;
    while read < n {
        match f.read(&mut buf[read..])? {
            0 => break,
            m => read += m,
        }
    }
    buf.truncate(read);
    Ok(buf)
}

fn is_encrypted(path: &Path) -> bool {
    match read_file_head(path, 16) {
        Ok(bytes) if bytes.len() == 16 => bytes.as_slice() != SQLITE_MAGIC.as_slice(),
        _ => false,
    }
}

fn create_profile(path: &Path, password: &str) -> Result<(), String> {
    // save dialog already confirmed replace intent if the file existed
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }

    if password.is_empty() {
        let result = (|| -> Result<(), String> {
            let db = Db::open(path, None, None)?;
            db.init_schema()?;
            db.add_group("Main")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(path);
        }
        return result;
    }

    let salt = crypto::generate_salt();
    let mut key = crypto::derive_key(password, &salt)?;

    let result = (|| -> Result<(), String> {
        let db = Db::open(path, Some(&key), Some(&salt))?;
        db.init_schema()?;
        db.add_group("Main")?;
        Ok(())
    })();
    key.zeroize();

    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

fn open_profile(path: &Path, password: &str) -> Result<(Db, Option<[u8; SALT_LEN]>), String> {
    if !is_encrypted(path) {
        let db = Db::open(path, None, None)?;
        db.init_schema()?;
        return Ok((db, None));
    }
    let salt_vec = read_file_head(path, SALT_LEN).map_err(|e| format!("read salt: {e}"))?;
    if salt_vec.len() != SALT_LEN {
        return Err("file too short to contain salt".into());
    }
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&salt_vec);
    let mut key = crypto::derive_key(password, &salt)?;
    let result = Db::open(path, Some(&key), None);
    key.zeroize();
    let db = result?;
    db.init_schema()?;
    Ok((db, Some(salt)))
}

fn enter_main(db_path: PathBuf, db: Db, salt: Option<[u8; SALT_LEN]>) -> MainState {
    let groups = db.list_groups().unwrap_or_default();
    let selected_group = groups.first().map(|g| g.id);
    let accounts = match selected_group {
        Some(gid) => db.list_accounts(gid).unwrap_or_default(),
        None => vec![],
    };

    let prefs = db.load_prefs().unwrap_or_default();
    let parse_width = |v: &String| v.parse::<f32>().ok();
    let site_width = prefs
        .get(PREF_COL_SITE)
        .and_then(parse_width)
        .unwrap_or(DEFAULT_SITE_WIDTH);
    let mut field_widths = HashMap::new();
    for (k, v) in &prefs {
        if let Some(field_key) = k.strip_prefix(PREF_COL_FIELD_PREFIX) {
            if let Some(w) = parse_width(v) {
                field_widths.insert(field_key.to_string(), w);
            }
        }
    }

    let quick_add = match prefs.get(PREF_QUICK_ADD) {
        Some(s) => decode_quick_add(s),
        None => DEFAULT_QUICK_ADD.iter().map(|s| s.to_string()).collect(),
    };

    let auto_lock = prefs
        .get(PREF_AUTO_LOCK)
        .map(|s| AutoLockTimeout::decode(s))
        .unwrap_or(AutoLockTimeout::Never);

    MainState {
        db_path,
        db,
        salt,
        groups,
        selected_group,
        accounts,
        new_group_name: String::new(),
        search: String::new(),
        editor: None,
        error: None,
        renaming_group: None,
        group_menu_open: None,
        settings: None,
        site_width,
        field_widths,
        quick_add,
        auto_lock,
        last_activity: Instant::now(),
    }
}

fn display_name(p: &Path) -> String {
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("profile")
        .to_string()
}

// ---------- views ----------

fn start_view(error: Option<&str>) -> Element<'_, Message> {
    let mut c = column![
        text("Account Manager").size(32),
        vertical_space().height(Length::Fixed(8.0)),
        button(text("Open Profile...").size(15).center())
            .padding(12)
            .on_press(Message::PickOpenPath)
            .width(Length::Fill)
            .style(button::primary),
        button(text("New Profile...").size(15).center())
            .padding(12)
            .on_press(Message::PickNewPath)
            .width(Length::Fill)
            .style(button::secondary),
    ]
    .spacing(10);

    if let Some(e) = error {
        c = c.push(error_text(e));
    }

    let card = container(c)
        .padding(28)
        .width(Length::Fixed(420.0))
        .style(container::rounded_box);

    container(card)
        .padding(24)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn create_profile_view(st: &CreateProfileState) -> Element<'_, Message> {
    let mut c = column![
        text("Create Profile").size(26),
        text(format!("File: {}", st.db_path.display()))
            .size(12)
            .color(MUTED),
        vertical_space().height(Length::Fixed(6.0)),
        text_input("Master password (leave blank for unencrypted)", &st.password)
            .on_input(Message::CreatePasswordChanged)
            .secure(true)
            .padding(10),
        text_input("Confirm password", &st.confirm)
            .on_input(Message::CreateConfirmChanged)
            .on_submit(Message::CreateSubmit)
            .secure(true)
            .padding(10),
    ]
    .spacing(10);
    if let Some(e) = &st.error {
        c = c.push(error_text(e));
    }
    c = c.push(
        row![
            button(text("Create").size(14))
                .padding([8, 18])
                .on_press(Message::CreateSubmit)
                .style(button::primary),
            button(text("Cancel").size(14))
                .padding([8, 18])
                .on_press(Message::CreateCancel)
                .style(button::secondary),
        ]
        .spacing(10),
    );

    let card = container(c)
        .padding(28)
        .width(Length::Fixed(520.0))
        .style(container::rounded_box);

    container(card)
        .padding(24)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn unlock_view(st: &UnlockState) -> Element<'_, Message> {
    let mut c = column![
        text(format!("Unlock '{}'", display_name(&st.db_path))).size(26),
        text(format!("File: {}", st.db_path.display()))
            .size(12)
            .color(MUTED),
        vertical_space().height(Length::Fixed(6.0)),
        text_input("Password", &st.password)
            .on_input(Message::UnlockPasswordChanged)
            .on_submit(Message::UnlockSubmit)
            .secure(true)
            .padding(10),
    ]
    .spacing(10);
    if let Some(e) = &st.error {
        c = c.push(error_text(e));
    }
    c = c.push(
        row![
            button(text("Unlock").size(14))
                .padding([8, 18])
                .on_press(Message::UnlockSubmit)
                .style(button::primary),
            button(text("Back").size(14))
                .padding([8, 18])
                .on_press(Message::UnlockCancel)
                .style(button::secondary),
        ]
        .spacing(10),
    );

    let card = container(c)
        .padding(28)
        .width(Length::Fixed(520.0))
        .style(container::rounded_box);

    container(card)
        .padding(24)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
}

fn main_view(st: &MainState) -> Element<'_, Message> {
    let mut groups_col = column![
        text("GROUPS").size(11).color(MUTED),
        vertical_space().height(Length::Fixed(4.0)),
    ]
    .spacing(4);

    for g in &st.groups {
        let renaming = st
            .renaming_group
            .as_ref()
            .filter(|(rid, _)| *rid == g.id)
            .map(|(_, name)| name.as_str());

        if let Some(current) = renaming {
            groups_col = groups_col.push(
                row![
                    text_input("Name", current)
                        .on_input(Message::RenameGroupChanged)
                        .on_submit(Message::ConfirmRenameGroup)
                        .padding(6)
                        .size(13),
                    button(text("OK").size(11))
                        .padding([4, 8])
                        .on_press(Message::ConfirmRenameGroup)
                        .style(button::primary),
                    button(text("Cancel").size(11))
                        .padding([4, 8])
                        .on_press(Message::CancelRenameGroup)
                        .style(button::secondary),
                ]
                .spacing(4)
                .align_y(Alignment::Center),
            );
            continue;
        }

        let selected = st.selected_group == Some(g.id);
        let mut name_btn = button(text(g.name.clone()).size(14))
            .width(Length::Fill)
            .padding([6, 10])
            .on_press(Message::SelectGroup(g.id));
        name_btn = if selected {
            name_btn.style(button::primary)
        } else {
            name_btn.style(button::text)
        };
        let menu_open = st.group_menu_open == Some(g.id);
        let actions: Element<Message> = if menu_open {
            row![
                button(text("Delete").size(11))
                    .padding([4, 8])
                    .on_press(Message::DeleteGroup(g.id))
                    .style(button::danger),
                button(text("Cancel").size(11))
                    .padding([4, 8])
                    .on_press(Message::ToggleGroupMenu(g.id))
                    .style(button::secondary),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![
                button(text("Edit").size(11))
                    .padding([4, 8])
                    .on_press(Message::StartRenameGroup(g.id))
                    .style(button::secondary),
                button(text("…").size(11))
                    .padding([4, 8])
                    .on_press(Message::ToggleGroupMenu(g.id))
                    .style(button::secondary),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        };
        groups_col = groups_col.push(
            row![name_btn, actions]
                .spacing(4)
                .align_y(Alignment::Center),
        );
    }
    groups_col = groups_col.push(vertical_space().height(Length::Fixed(8.0)));
    groups_col = groups_col.push(horizontal_rule(1));
    groups_col = groups_col.push(vertical_space().height(Length::Fixed(4.0)));
    groups_col = groups_col.push(
        row![
            text_input("New group", &st.new_group_name)
                .on_input(Message::NewGroupNameChanged)
                .on_submit(Message::AddGroup)
                .padding(8),
            button(text("+").size(14))
                .padding([6, 12])
                .on_press(Message::AddGroup)
                .style(button::primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );

    let sidebar = container(scrollable(groups_col).height(Length::Fill))
        .width(Length::Fixed(240.0))
        .height(Length::Fill)
        .padding(16)
        .style(container::bordered_box);

    let body: Element<Message> = if let Some(ss) = &st.settings {
        settings_view(ss, st.auto_lock, st.salt.is_some(), &st.quick_add)
    } else if let Some(editor) = &st.editor {
        editor_view(editor, st.error.as_deref(), &st.quick_add)
    } else {
        accounts_view(st)
    };

    let header = container(
        row![
            text(format!("Profile: {}", display_name(&st.db_path)))
                .size(18),
            horizontal_space(),
            button(text("Settings").size(13))
                .padding([6, 14])
                .on_press(Message::OpenSettings)
                .style(button::secondary),
            button(text("Lock").size(13))
                .padding([6, 14])
                .on_press(Message::LockProfile)
                .style(button::secondary),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([12, 18]);

    column![
        header,
        horizontal_rule(1),
        row![
            sidebar,
            container(body)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(20),
        ]
    ]
    .into()
}

fn accounts_view(st: &MainState) -> Element<'_, Message> {
    let has_group = st.selected_group.is_some();
    let add_btn = button(text("+ Add Account").size(13))
        .padding([8, 14])
        .on_press_maybe(has_group.then_some(Message::NewAccount))
        .style(button::primary);

    let header = row![text("Accounts").size(22), horizontal_space(), add_btn]
        .align_y(Alignment::Center);

    let search_bar = text_input("Search accounts…", &st.search)
        .on_input(Message::SearchChanged)
        .padding(10)
        .size(14);

    let q = st.search.trim().to_lowercase();
    let filtered: Vec<&Account> = if q.is_empty() {
        st.accounts.iter().collect()
    } else {
        st.accounts
            .iter()
            .filter(|a| {
                a.site.to_lowercase().contains(&q)
                    || a.fields.iter().any(|f| {
                        f.key.to_lowercase().contains(&q)
                            || f.value.to_lowercase().contains(&q)
                    })
            })
            .collect()
    };

    let body: Element<Message> = if !has_group {
        empty_state("Select or create a group to get started.")
    } else if st.accounts.is_empty() {
        empty_state("No accounts yet. Click \"+ Add Account\" to create one.")
    } else if filtered.is_empty() {
        empty_state("No accounts match your search.")
    } else {
        accounts_table(st, &filtered)
    };

    column![header, search_bar, body].spacing(14).into()
}

fn resize_buttons(col: ColumnId) -> Element<'static, Message> {
    row![
        button(text("−").size(10))
            .padding([1, 5])
            .on_press(Message::ResizeColumn(col.clone(), -(COLUMN_STEP as i32)))
            .style(button::secondary),
        button(text("+").size(10))
            .padding([1, 5])
            .on_press(Message::ResizeColumn(col, COLUMN_STEP as i32))
            .style(button::secondary),
    ]
    .spacing(2)
    .align_y(Alignment::Center)
    .into()
}

fn header_cell(label: String, width: Length, col: ColumnId) -> Element<'static, Message> {
    container(
        row![
            text(label).size(12).color(MUTED).width(Length::Fill),
            resize_buttons(col),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .width(width)
    .into()
}

fn accounts_table<'a>(st: &'a MainState, accounts: &[&'a Account]) -> Element<'a, Message> {
    let mut keys: Vec<String> = Vec::new();
    for a in accounts {
        for f in &a.fields {
            let k = f.key.trim();
            if !k.is_empty() && !keys.iter().any(|e| e == k) {
                keys.push(k.to_string());
            }
        }
    }
    keys.sort();

    let site_len = Length::Fixed(st.site_width);
    let actions_len = Length::Fixed(ACTIONS_WIDTH);
    let field_len = |k: &str| -> Length {
        Length::Fixed(
            *st.field_widths
                .get(k)
                .unwrap_or(&DEFAULT_FIELD_WIDTH),
        )
    };

    let mut header_row =
        row![header_cell("Site".to_string(), site_len, ColumnId::Site)].spacing(10);
    for k in &keys {
        header_row = header_row.push(header_cell(
            k.clone(),
            field_len(k),
            ColumnId::Field(k.clone()),
        ));
    }
    header_row = header_row.push(container(text("")).width(actions_len));

    let mut body_col = column![container(header_row).padding([6, 10])].spacing(0);

    for (i, a) in accounts.iter().enumerate() {
        let site_label = if a.pinned {
            format!("★ {}", a.site)
        } else {
            a.site.clone()
        };
        let mut r = row![text(site_label).size(13).width(site_len)]
            .spacing(10)
            .align_y(Alignment::Center);
        for k in &keys {
            let joined = a
                .fields
                .iter()
                .filter(|f| f.key.trim() == k.as_str())
                .map(|f| f.value.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            r = r.push(text(joined).size(13).width(field_len(k)));
        }
        let pin_label = if a.pinned { "Unpin" } else { "Pin" };
        r = r.push(
            row![
                button(text(pin_label).size(11))
                    .padding([4, 8])
                    .on_press(Message::TogglePin(a.id))
                    .style(if a.pinned {
                        button::primary
                    } else {
                        button::secondary
                    }),
                button(text("Edit").size(11))
                    .padding([4, 8])
                    .on_press(Message::EditAccount(a.id))
                    .style(button::secondary),
                button(text("Del").size(11))
                    .padding([4, 8])
                    .on_press(Message::DeleteAccount(a.id))
                    .style(button::danger),
            ]
            .spacing(4)
            .width(actions_len),
        );

        let mut row_c = container(r).padding([10, 10]).width(Length::Shrink);
        if a.pinned || i % 2 == 1 {
            row_c = row_c.style(container::rounded_box);
        }
        body_col = body_col.push(row_c);
    }

    scrollable(body_col)
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::default(),
            horizontal: scrollable::Scrollbar::default(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn empty_state(msg: &str) -> Element<'_, Message> {
    container(text(msg.to_string()).size(14).color(MUTED))
        .padding(32)
        .center_x(Length::Fill)
        .into()
}

fn editor_view<'a>(
    e: &'a AccountEditor,
    error: Option<&'a str>,
    presets: &'a [String],
) -> Element<'a, Message> {
    let title = if e.id == 0 { "New Account" } else { "Edit Account" };

    let mut col = column![
        text(title).size(24),
        vertical_space().height(Length::Fixed(2.0)),
        text("SITE").size(11).color(MUTED),
        text_input("e.g. Netflix", &e.site)
            .id(site_input_id())
            .on_input(Message::EditSite)
            .on_submit(Message::EditFocusNext(FocusFrom::Site))
            .padding(10)
            .size(14),
        vertical_space().height(Length::Fixed(6.0)),
        text("FIELDS").size(11).color(MUTED),
    ]
    .spacing(6);

    if let Some(err) = error {
        col = col.push(error_text(err));
    }

    for (i, f) in e.fields.iter().enumerate() {
        col = col.push(
            row![
                text_input("Key (e.g. email)", &f.key)
                    .id(key_input_id(i))
                    .on_input(move |s| Message::EditFieldKey(i, s))
                    .on_submit(Message::EditFocusNext(FocusFrom::Key(i)))
                    .width(Length::FillPortion(2))
                    .padding(8)
                    .size(13),
                text_input("Value", &f.value)
                    .id(value_input_id(i))
                    .on_input(move |s| Message::EditFieldValue(i, s))
                    .on_submit(Message::EditFocusNext(FocusFrom::Value(i)))
                    .width(Length::FillPortion(3))
                    .padding(8)
                    .size(13),
                button(text("×").size(14))
                    .padding([4, 10])
                    .on_press(Message::RemoveField(i))
                    .style(button::danger),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
    }

    if !presets.is_empty() {
        col = col.push(vertical_space().height(Length::Fixed(4.0)));
        col = col.push(text("QUICK ADD").size(11).color(MUTED));
        let mut chips = row![].spacing(6).align_y(Alignment::Center);
        for preset in presets {
            chips = chips.push(
                button(text(preset.clone()).size(11))
                    .padding([4, 10])
                    .on_press(Message::AddFieldWithKey(preset.clone()))
                    .style(button::secondary),
            );
        }
        col = col.push(
            scrollable(chips).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            )),
        );
    }

    col = col.push(
        button(text("+ Add Field").size(13))
            .padding([6, 14])
            .on_press(Message::AddField)
            .style(button::secondary),
    );

    col = col.push(vertical_space().height(Length::Fixed(8.0)));
    col = col.push(horizontal_rule(1));
    col = col.push(
        row![
            button(text("Save").size(14))
                .padding([8, 20])
                .on_press(Message::EditSave)
                .style(button::primary),
            button(text("Cancel").size(14))
                .padding([8, 20])
                .on_press(Message::EditCancel)
                .style(button::secondary),
        ]
        .spacing(10),
    );

    let card = container(col.max_width(640))
        .padding(24)
        .width(Length::Fill)
        .style(container::rounded_box);

    scrollable(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

fn settings_view<'a>(
    ss: &'a SettingsState,
    auto_lock: AutoLockTimeout,
    encrypted: bool,
    quick_add: &'a [String],
) -> Element<'a, Message> {
    let mut col = column![
        text("Settings").size(26),
        vertical_space().height(Length::Fixed(8.0)),
        text("AUTO-LOCK ON IDLE").size(11).color(MUTED),
        vertical_space().height(Length::Fixed(2.0)),
    ]
    .spacing(6);

    for t in AutoLockTimeout::ALL {
        col = col.push(radio(
            t.label(),
            *t,
            Some(auto_lock),
            Message::AutoLockChanged,
        ));
    }

    col = col.push(vertical_space().height(Length::Fixed(14.0)));
    col = col.push(horizontal_rule(1));
    col = col.push(vertical_space().height(Length::Fixed(8.0)));
    col = col.push(text("CHANGE MASTER PASSWORD").size(11).color(MUTED));

    if encrypted {
        col = col.push(
            text_input("New password", &ss.new_password)
                .on_input(Message::SettingsNewPasswordChanged)
                .secure(true)
                .padding(10),
        );
        col = col.push(
            text_input("Confirm new password", &ss.confirm)
                .on_input(Message::SettingsConfirmPasswordChanged)
                .on_submit(Message::ChangePasswordSubmit)
                .secure(true)
                .padding(10),
        );
        if let Some(err) = &ss.error {
            col = col.push(error_text(err));
        }
        if let Some(ok) = &ss.success {
            col = col.push(text(ok.clone()).size(13).color(SUCCESS));
        }
        col = col.push(
            button(text("Change password").size(14))
                .padding([8, 18])
                .on_press(Message::ChangePasswordSubmit)
                .style(button::primary),
        );
    } else {
        col = col.push(
            text("This profile is unencrypted. Create a new encrypted profile to set a master password.")
                .size(13)
                .color(MUTED),
        );
    }

    col = col.push(vertical_space().height(Length::Fixed(14.0)));
    col = col.push(horizontal_rule(1));
    col = col.push(vertical_space().height(Length::Fixed(8.0)));
    col = col.push(
        row![
            text("QUICK ADD PRESETS").size(11).color(MUTED),
            horizontal_space(),
            button(text("Reset to defaults").size(11))
                .padding([4, 10])
                .on_press(Message::ResetQuickAddDefaults)
                .style(button::secondary),
        ]
        .align_y(Alignment::Center),
    );

    if quick_add.is_empty() {
        col = col.push(text("No presets. Add one below.").size(12).color(MUTED));
    } else {
        for (i, preset) in quick_add.iter().enumerate() {
            col = col.push(
                row![
                    text(preset.clone()).size(13).width(Length::Fill),
                    button(text("Remove").size(11))
                        .padding([4, 10])
                        .on_press(Message::RemoveQuickAddPreset(i))
                        .style(button::danger),
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            );
        }
    }

    col = col.push(
        row![
            text_input("New preset (e.g. API key)", &ss.quick_add_input)
                .on_input(Message::QuickAddInputChanged)
                .on_submit(Message::AddQuickAddPreset)
                .padding(8),
            button(text("Add").size(13))
                .padding([6, 14])
                .on_press(Message::AddQuickAddPreset)
                .style(button::primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    );

    col = col.push(vertical_space().height(Length::Fixed(14.0)));
    col = col.push(horizontal_rule(1));
    col = col.push(
        button(text("Back").size(14))
            .padding([8, 18])
            .on_press(Message::CloseSettings)
            .style(button::secondary),
    );

    let card = container(col.max_width(560))
        .padding(24)
        .width(Length::Fill)
        .style(container::rounded_box);

    scrollable(card)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ---------- shared view helpers ----------

const MUTED: Color = Color {
    r: 0.78,
    g: 0.78,
    b: 0.84,
    a: 1.0,
};

const DANGER: Color = Color {
    r: 1.0,
    g: 0.45,
    b: 0.48,
    a: 1.0,
};

const SUCCESS: Color = Color {
    r: 0.45,
    g: 0.85,
    b: 0.55,
    a: 1.0,
};

fn error_text(msg: &str) -> Element<'_, Message> {
    text(msg.to_string()).size(13).color(DANGER).into()
}
