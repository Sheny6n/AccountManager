mod crypto;
mod db;
mod model;

use std::io::Read;
use std::path::{Path, PathBuf};

use iced::widget::{
    button, column, container, horizontal_rule, horizontal_space, row, scrollable, text,
    text_input, vertical_space,
};
use iced::{Alignment, Color, Element, Length, Task, Theme};
use zeroize::Zeroize;

use db::Db;
use model::{Account, Field, Group};

fn main() -> iced::Result {
    iced::application("Account Manager", App::update, App::view)
        .theme(|_| Theme::TokyoNight)
        .window_size((1080.0, 720.0))
        .run_with(App::new)
}

struct App {
    screen: Screen,
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
    groups: Vec<Group>,
    selected_group: Option<i64>,
    accounts: Vec<Account>,
    new_group_name: String,
    search: String,
    editor: Option<AccountEditor>,
    error: Option<String>,
}

#[derive(Default)]
struct AccountEditor {
    id: i64,
    site: String,
    fields: Vec<Field>,
}

#[derive(Debug, Clone)]
enum Message {
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
}

impl App {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                screen: Screen::Start,
                startup_error: None,
            },
            Task::none(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PickOpenPath => {
                if let Some(path) = pick_open_path() {
                    self.startup_error = None;
                    if is_encrypted(&path) {
                        self.screen = Screen::Unlock(UnlockState {
                            db_path: path,
                            password: String::new(),
                            error: None,
                        });
                    } else {
                        match open_profile(&path, "") {
                            Ok(db) => self.screen = Screen::Main(enter_main(path, db)),
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
                    self.screen = Screen::CreateProfile(CreateProfileState {
                        db_path: path,
                        password: String::new(),
                        confirm: String::new(),
                        error: None,
                    });
                }
            }

            Message::CreatePasswordChanged(s) => {
                if let Screen::CreateProfile(st) = &mut self.screen {
                    st.password = s;
                }
            }
            Message::CreateConfirmChanged(s) => {
                if let Screen::CreateProfile(st) = &mut self.screen {
                    st.confirm = s;
                }
            }
            Message::CreateSubmit => {
                if let Screen::CreateProfile(st) = &mut self.screen {
                    if !st.password.is_empty() && st.password != st.confirm {
                        st.error = Some("Passwords don't match".into());
                        return Task::none();
                    }
                    let path = st.db_path.clone();
                    match create_profile(&path, &st.password) {
                        Ok(()) => {
                            st.password.zeroize();
                            st.confirm.zeroize();
                            if is_encrypted(&path) {
                                self.screen = Screen::Unlock(UnlockState {
                                    db_path: path,
                                    password: String::new(),
                                    error: None,
                                });
                            } else {
                                match open_profile(&path, "") {
                                    Ok(db) => {
                                        self.screen = Screen::Main(enter_main(path, db));
                                    }
                                    Err(e) => st.error = Some(e),
                                }
                            }
                        }
                        Err(e) => st.error = Some(e),
                    }
                }
            }
            Message::CreateCancel => self.screen = Screen::Start,

            Message::UnlockPasswordChanged(s) => {
                if let Screen::Unlock(st) = &mut self.screen {
                    st.password = s;
                }
            }
            Message::UnlockSubmit => {
                if let Screen::Unlock(st) = &mut self.screen {
                    match open_profile(&st.db_path, &st.password) {
                        Ok(db) => {
                            st.password.zeroize();
                            let path = std::mem::take(&mut st.db_path);
                            self.screen = Screen::Main(enter_main(path, db));
                        }
                        Err(e) => st.error = Some(e),
                    }
                }
            }
            Message::UnlockCancel => self.screen = Screen::Start,

            Message::LockProfile => self.screen = Screen::Start,
            Message::SelectGroup(id) => {
                if let Screen::Main(st) = &mut self.screen {
                    st.selected_group = Some(id);
                    st.accounts = st.db.list_accounts(id).unwrap_or_default();
                    st.editor = None;
                    st.search.clear();
                }
            }
            Message::SearchChanged(s) => {
                if let Screen::Main(st) = &mut self.screen {
                    st.search = s;
                }
            }
            Message::NewGroupNameChanged(s) => {
                if let Screen::Main(st) = &mut self.screen {
                    st.new_group_name = s;
                }
            }
            Message::AddGroup => {
                if let Screen::Main(st) = &mut self.screen {
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
                if let Screen::Main(st) = &mut self.screen {
                    let _ = st.db.delete_group(id);
                    st.groups = st.db.list_groups().unwrap_or_default();
                    if st.selected_group == Some(id) {
                        st.selected_group = st.groups.first().map(|g| g.id);
                    }
                    st.accounts = match st.selected_group {
                        Some(gid) => st.db.list_accounts(gid).unwrap_or_default(),
                        None => vec![],
                    };
                }
            }

            Message::NewAccount => {
                if let Screen::Main(st) = &mut self.screen {
                    if st.selected_group.is_some() {
                        st.editor = Some(AccountEditor {
                            fields: vec![Field::default()],
                            ..Default::default()
                        });
                    }
                }
            }
            Message::EditAccount(id) => {
                if let Screen::Main(st) = &mut self.screen {
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
                if let Screen::Main(st) = &mut self.screen {
                    let _ = st.db.delete_account(id);
                    if let Some(gid) = st.selected_group {
                        st.accounts = st.db.list_accounts(gid).unwrap_or_default();
                    }
                }
            }

            Message::EditSite(s) => edit_editor(&mut self.screen, |e| e.site = s),
            Message::EditFieldKey(idx, s) => edit_editor(&mut self.screen, |e| {
                if let Some(f) = e.fields.get_mut(idx) {
                    f.key = s;
                }
            }),
            Message::EditFieldValue(idx, s) => edit_editor(&mut self.screen, |e| {
                if let Some(f) = e.fields.get_mut(idx) {
                    f.value = s;
                }
            }),
            Message::AddField => edit_editor(&mut self.screen, |e| {
                e.fields.push(Field::default());
            }),
            Message::RemoveField(idx) => edit_editor(&mut self.screen, |e| {
                if idx < e.fields.len() {
                    e.fields.remove(idx);
                }
            }),
            Message::EditSave => {
                if let Screen::Main(st) = &mut self.screen {
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
                if let Screen::Main(st) = &mut self.screen {
                    st.editor = None;
                    st.error = None;
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        match &self.screen {
            Screen::Start => start_view(self.startup_error.as_deref()),
            Screen::CreateProfile(st) => create_profile_view(st),
            Screen::Unlock(st) => unlock_view(st),
            Screen::Main(st) => main_view(st),
        }
    }
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

fn open_profile(path: &Path, password: &str) -> Result<Db, String> {
    let db = if !is_encrypted(path) {
        Db::open(path, None, None)?
    } else {
        let salt = read_file_head(path, 16).map_err(|e| format!("read salt: {e}"))?;
        if salt.len() != 16 {
            return Err("file too short to contain salt".into());
        }
        let mut key = crypto::derive_key(password, &salt)?;
        let result = Db::open(path, Some(&key), None);
        key.zeroize();
        result?
    };
    db.init_schema()?;
    Ok(db)
}

fn enter_main(db_path: PathBuf, db: Db) -> MainState {
    let groups = db.list_groups().unwrap_or_default();
    let selected_group = groups.first().map(|g| g.id);
    let accounts = match selected_group {
        Some(gid) => db.list_accounts(gid).unwrap_or_default(),
        None => vec![],
    };
    MainState {
        db_path,
        db,
        groups,
        selected_group,
        accounts,
        new_group_name: String::new(),
        search: String::new(),
        editor: None,
        error: None,
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
        text("A profile is a .db file you choose the location for.")
            .size(13)
            .color(MUTED),
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

    let body: Element<Message> = if let Some(editor) = &st.editor {
        editor_view(editor, st.error.as_deref())
    } else {
        accounts_view(&st.accounts, &st.search, st.selected_group.is_some())
    };

    let header = container(
        row![
            text(format!("Profile: {}", display_name(&st.db_path)))
                .size(18),
            horizontal_space(),
            button(text("Lock").size(13))
                .padding([6, 14])
                .on_press(Message::LockProfile)
                .style(button::secondary),
        ]
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

// ---------- shared view helpers ----------

const MUTED: Color = Color {
    r: 0.6,
    g: 0.6,
    b: 0.68,
    a: 1.0,
};

const DANGER: Color = Color {
    r: 0.92,
    g: 0.36,
    b: 0.36,
    a: 1.0,
};

fn error_text(msg: &str) -> Element<'_, Message> {
    text(msg.to_string()).size(13).color(DANGER).into()
}
