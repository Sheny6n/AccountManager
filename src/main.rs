mod config;
mod crypto;
mod db;
mod model;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, radio, row, scrollable, text,
    text_input, vertical_space,
};
use iced::{Alignment, Color, Element, Length, Subscription, Task, Theme};
use zeroize::Zeroize;

use config::{AppConfig, AutoLockTimeout};
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
    config: AppConfig,
    last_activity: Instant,
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
    settings: Option<SettingsState>,
}

#[derive(Default)]
struct SettingsState {
    new_password: String,
    confirm: String,
    error: Option<String>,
    success: Option<String>,
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
    SearchChanged(String),

    NewAccount,
    EditAccount(i64),
    DeleteAccount(i64),

    EditSite(String),
    EditFieldKey(usize, String),
    EditFieldValue(usize, String),
    AddField,
    RemoveField(usize),
    EditSave,
    EditCancel,

    OpenSettings,
    CloseSettings,
    AutoLockChanged(AutoLockTimeout),
    SettingsNewPasswordChanged(String),
    SettingsConfirmPasswordChanged(String),
    ChangePasswordSubmit,

    Tick,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                tabs: vec![Screen::Start],
                active_tab: 0,
                startup_error: None,
                config: AppConfig::load(),
                last_activity: Instant::now(),
            },
            Task::none(),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        if self.config.auto_lock.seconds().is_some() {
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
            self.last_activity = Instant::now();
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
                }
            }
            Message::StartRenameGroup(id) => {
                if let Screen::Main(st) = self.active_mut() {
                    if let Some(g) = st.groups.iter().find(|g| g.id == id) {
                        st.renaming_group = Some((id, g.name.clone()));
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
                        let a = Account {
                            id: e.id,
                            group_id: gid,
                            site: e.site.clone(),
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
                self.config.auto_lock = t;
                let _ = self.config.save();
                self.last_activity = Instant::now();
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
                if let Some(secs) = self.config.auto_lock.seconds() {
                    if self.last_activity.elapsed() >= Duration::from_secs(secs) {
                        for s in &mut self.tabs {
                            if matches!(s, Screen::Main(_)) {
                                *s = Screen::Start;
                            }
                        }
                        self.last_activity = Instant::now();
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
            Screen::Main(st) => main_view(st, &self.config),
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
        settings: None,
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

fn main_view<'a>(st: &'a MainState, config: &'a AppConfig) -> Element<'a, Message> {
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
        groups_col = groups_col.push(
            row![
                name_btn,
                button(text("Edit").size(11))
                    .padding([4, 8])
                    .on_press(Message::StartRenameGroup(g.id))
                    .style(button::secondary),
                button(text("×").size(14))
                    .padding([4, 8])
                    .on_press(Message::DeleteGroup(g.id))
                    .style(button::danger),
            ]
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
        settings_view(ss, config, st.salt.is_some())
    } else if let Some(editor) = &st.editor {
        editor_view(editor, st.error.as_deref())
    } else {
        accounts_view(&st.accounts, &st.search, st.selected_group.is_some())
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

fn accounts_view<'a>(
    accounts: &'a [Account],
    search: &'a str,
    has_group: bool,
) -> Element<'a, Message> {
    let add_btn = button(text("+ Add Account").size(13))
        .padding([8, 14])
        .on_press_maybe(has_group.then_some(Message::NewAccount))
        .style(button::primary);

    let header = row![
        text("Accounts").size(22),
        horizontal_space(),
        add_btn,
    ]
    .align_y(Alignment::Center);

    let search_bar = text_input("Search accounts…", search)
        .on_input(Message::SearchChanged)
        .padding(10)
        .size(14);

    let q = search.trim().to_lowercase();
    let filtered: Vec<&Account> = if q.is_empty() {
        accounts.iter().collect()
    } else {
        accounts
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

    let body: Element<'a, Message> = if !has_group {
        empty_state("Select or create a group to get started.")
    } else if accounts.is_empty() {
        empty_state("No accounts yet. Click \"+ Add Account\" to create one.")
    } else if filtered.is_empty() {
        empty_state("No accounts match your search.")
    } else {
        accounts_table(&filtered)
    };

    column![header, search_bar, body].spacing(14).into()
}

fn accounts_table<'a>(accounts: &[&'a Account]) -> Element<'a, Message> {
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

    let site_w = Length::Fixed(200.0);
    let field_w = Length::Fixed(180.0);
    let actions_w = Length::Fixed(120.0);

    let mut header_row = row![text("Site").size(12).color(MUTED).width(site_w)].spacing(10);
    for k in &keys {
        header_row = header_row.push(text(k.clone()).size(12).color(MUTED).width(field_w));
    }
    header_row = header_row.push(text("").width(actions_w));

    let mut body_col = column![container(header_row).padding([6, 10])].spacing(0);

    for (i, a) in accounts.iter().enumerate() {
        let mut r = row![text(a.site.clone()).size(13).width(site_w)]
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
            r = r.push(text(joined).size(13).width(field_w));
        }
        r = r.push(
            row![
                button(text("Edit").size(11))
                    .padding([4, 10])
                    .on_press(Message::EditAccount(a.id))
                    .style(button::secondary),
                button(text("Del").size(11))
                    .padding([4, 10])
                    .on_press(Message::DeleteAccount(a.id))
                    .style(button::danger),
            ]
            .spacing(4)
            .width(actions_w),
        );

        let mut row_c = container(r).padding([10, 10]).width(Length::Shrink);
        if i % 2 == 1 {
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

fn editor_view<'a>(e: &'a AccountEditor, error: Option<&'a str>) -> Element<'a, Message> {
    let title = if e.id == 0 { "New Account" } else { "Edit Account" };

    let mut col = column![
        text(title).size(24),
        vertical_space().height(Length::Fixed(2.0)),
        text("SITE").size(11).color(MUTED),
        text_input("e.g. Netflix", &e.site)
            .on_input(Message::EditSite)
            .on_submit(Message::EditSave)
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
                    .on_input(move |s| Message::EditFieldKey(i, s))
                    .width(Length::FillPortion(2))
                    .padding(8)
                    .size(13),
                text_input("Value", &f.value)
                    .on_input(move |s| Message::EditFieldValue(i, s))
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
    config: &'a AppConfig,
    encrypted: bool,
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
            Some(config.auto_lock),
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
