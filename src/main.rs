mod crypto;
mod db;
mod model;
mod paths;

use std::path::{Path, PathBuf};

use iced::widget::{
    button, column, container, horizontal_space, row, scrollable, text, text_input,
};
use iced::{Alignment, Color, Element, Length, Task};
use zeroize::Zeroize;

use db::Db;
use model::{Account, Group};

fn main() -> iced::Result {
    iced::application("Account Manager", App::update, App::view)
        .window_size((960.0, 640.0))
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
}

#[derive(Default)]
struct AccountEditor {
    id: i64,
    site: String,
    email: String,
    region: String,
    payment_methods: String,
    notes: String,
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
    EditEmail(String),
    EditRegion(String),
    EditPayment(String),
    EditNotes(String),
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
                        st.editor = Some(AccountEditor::default());
                    }
                }
            }
            Message::EditAccount(id) => {
                if let Screen::Main(st) = &mut self.screen {
                    if let Some(a) = st.accounts.iter().find(|a| a.id == id) {
                        st.editor = Some(AccountEditor {
                            id: a.id,
                            site: a.site.clone(),
                            email: a.email.clone(),
                            region: a.region.clone(),
                            payment_methods: a.payment_methods.clone(),
                            notes: a.notes.clone(),
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

            Message::EditSite(s) => edit_field(&mut self.screen, |e| e.site = s),
            Message::EditEmail(s) => edit_field(&mut self.screen, |e| e.email = s),
            Message::EditRegion(s) => edit_field(&mut self.screen, |e| e.region = s),
            Message::EditPayment(s) => edit_field(&mut self.screen, |e| e.payment_methods = s),
            Message::EditNotes(s) => edit_field(&mut self.screen, |e| e.notes = s),
            Message::EditSave => {
                if let Screen::Main(st) = &mut self.screen {
                    if let (Some(gid), Some(e)) = (st.selected_group, &st.editor) {
                        let a = Account {
                            id: e.id,
                            group_id: gid,
                            site: e.site.clone(),
                            email: e.email.clone(),
                            region: e.region.clone(),
                            payment_methods: e.payment_methods.clone(),
                            notes: e.notes.clone(),
                        };
                        if !a.site.trim().is_empty() {
                            let _ = st.db.upsert_account(&a);
                            st.accounts = st.db.list_accounts(gid).unwrap_or_default();
                            st.editor = None;
                        }
                    }
                }
            }
            Message::EditCancel => {
                if let Screen::Main(st) = &mut self.screen {
                    st.editor = None;
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

fn edit_field(screen: &mut Screen, f: impl FnOnce(&mut AccountEditor)) {
    if let Screen::Main(st) = screen {
        if let Some(e) = st.editor.as_mut() {
            f(e);
        }
    }
}

// ---------- file dialogs ----------

fn pick_open_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Account Manager Profile", &["db"])
        .pick_file()
}

fn pick_new_path() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Account Manager Profile", &["db"])
        .set_file_name("profile.db")
        .save_file()
        .map(|p| {
            if p.extension().and_then(|s| s.to_str()) == Some("db") {
                p
            } else {
                p.with_extension("db")
            }
        })
}

// ---------- profile I/O ----------

fn is_encrypted(db_path: &Path) -> bool {
    paths::salt_path_for(db_path).exists()
}

fn create_profile(db_path: &Path, password: &str) -> Result<(), String> {
    let salt_path = paths::salt_path_for(db_path);
    // save dialog already confirmed replace intent if the file existed
    if db_path.exists() {
        std::fs::remove_file(db_path).map_err(|e| e.to_string())?;
    }
    if salt_path.exists() {
        std::fs::remove_file(&salt_path).map_err(|e| e.to_string())?;
    }

    if password.is_empty() {
        let result = (|| -> Result<(), String> {
            let db = Db::open(db_path, None)?;
            db.init_schema()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(db_path);
        }
        return result;
    }

    let salt = crypto::generate_salt();
    let mut key = crypto::derive_key(password, &salt)?;
    std::fs::write(&salt_path, salt).map_err(|e| e.to_string())?;

    let result = (|| -> Result<(), String> {
        let db = Db::open(db_path, Some(&key))?;
        db.init_schema()?;
        Ok(())
    })();
    key.zeroize();

    if result.is_err() {
        let _ = std::fs::remove_file(db_path);
        let _ = std::fs::remove_file(&salt_path);
    }
    result
}

fn open_profile(db_path: &Path, password: &str) -> Result<Db, String> {
    if !is_encrypted(db_path) {
        return Db::open(db_path, None);
    }
    let salt_path = paths::salt_path_for(db_path);
    let salt = std::fs::read(&salt_path).map_err(|e| format!("missing salt: {e}"))?;
    let mut key = crypto::derive_key(password, &salt)?;
    let result = Db::open(db_path, Some(&key));
    key.zeroize();
    result
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
        text("Account Manager").size(28),
        text("Each profile is a .db file you choose the location for.").size(13),
        button(text("Open Profile...").size(15))
            .on_press(Message::PickOpenPath)
            .width(Length::Fill),
        button(text("New Profile...").size(15))
            .on_press(Message::PickNewPath)
            .width(Length::Fill),
    ]
    .spacing(12)
    .max_width(420);

    if let Some(e) = error {
        c = c.push(text(e.to_string()).color(Color::from_rgb(0.85, 0.2, 0.2)));
    }

    container(c).padding(24).center_x(Length::Fill).into()
}

fn create_profile_view(st: &CreateProfileState) -> Element<'_, Message> {
    let mut c = column![
        text("Create Profile").size(24),
        text(format!("File: {}", st.db_path.display())).size(12),
        text_input("Master password (leave blank for unencrypted)", &st.password)
            .on_input(Message::CreatePasswordChanged)
            .secure(true)
            .padding(8),
        text_input("Confirm password", &st.confirm)
            .on_input(Message::CreateConfirmChanged)
            .on_submit(Message::CreateSubmit)
            .secure(true)
            .padding(8),
    ]
    .spacing(12);
    if let Some(e) = &st.error {
        c = c.push(text(e.clone()).color(Color::from_rgb(0.85, 0.2, 0.2)));
    }
    c = c.push(
        row![
            button(text("Create")).on_press(Message::CreateSubmit),
            button(text("Cancel")).on_press(Message::CreateCancel),
        ]
        .spacing(8),
    );
    container(column![c].max_width(520))
        .padding(24)
        .center_x(Length::Fill)
        .into()
}

fn unlock_view(st: &UnlockState) -> Element<'_, Message> {
    let mut c = column![
        text(format!("Unlock '{}'", display_name(&st.db_path))).size(24),
        text(format!("File: {}", st.db_path.display())).size(12),
        text_input("Password", &st.password)
            .on_input(Message::UnlockPasswordChanged)
            .on_submit(Message::UnlockSubmit)
            .secure(true)
            .padding(8),
    ]
    .spacing(12);
    if let Some(e) = &st.error {
        c = c.push(text(e.clone()).color(Color::from_rgb(0.85, 0.2, 0.2)));
    }
    c = c.push(
        row![
            button(text("Unlock")).on_press(Message::UnlockSubmit),
            button(text("Back")).on_press(Message::UnlockCancel),
        ]
        .spacing(8),
    );
    container(column![c].max_width(520))
        .padding(24)
        .center_x(Length::Fill)
        .into()
}

fn main_view(st: &MainState) -> Element<'_, Message> {
    let mut groups_col = column![text("Groups").size(16)].spacing(4);
    for g in &st.groups {
        let selected = st.selected_group == Some(g.id);
        let label = if selected {
            format!("▸ {}", g.name)
        } else {
            g.name.clone()
        };
        groups_col = groups_col.push(
            row![
                button(text(label).size(14))
                    .width(Length::Fill)
                    .on_press(Message::SelectGroup(g.id)),
                button(text("x").size(12)).on_press(Message::DeleteGroup(g.id)),
            ]
            .spacing(4),
        );
    }
    groups_col = groups_col.push(
        row![
            text_input("New group", &st.new_group_name)
                .on_input(Message::NewGroupNameChanged)
                .on_submit(Message::AddGroup)
                .padding(6),
            button(text("+")).on_press(Message::AddGroup),
        ]
        .spacing(4),
    );

    let sidebar = container(scrollable(groups_col))
        .width(Length::Fixed(220.0))
        .height(Length::Fill)
        .padding(12);

    let body: Element<Message> = if let Some(editor) = &st.editor {
        editor_view(editor)
    } else {
        accounts_view(&st.accounts, &st.search, st.selected_group.is_some())
    };

    let header = row![
        text(format!("Profile: {}", display_name(&st.db_path))).size(18),
        horizontal_space(),
        button(text("Lock")).on_press(Message::LockProfile),
    ]
    .align_y(Alignment::Center)
    .padding(8);

    column![
        header,
        row![
            sidebar,
            container(body).width(Length::Fill).height(Length::Fill).padding(12),
        ]
    ]
    .into()
}

fn accounts_view<'a>(
    accounts: &'a [Account],
    search: &'a str,
    has_group: bool,
) -> Element<'a, Message> {
    let add_btn = button(text("+ Add Account"))
        .on_press_maybe(has_group.then_some(Message::NewAccount));

    let header = row![text("Accounts").size(20), horizontal_space(), add_btn]
        .align_y(Alignment::Center);

    let search_bar = text_input("Search site, email, region, payment, notes", search)
        .on_input(Message::SearchChanged)
        .padding(8);

    let q = search.trim().to_lowercase();
    let filtered: Vec<&Account> = if q.is_empty() {
        accounts.iter().collect()
    } else {
        accounts
            .iter()
            .filter(|a| {
                a.site.to_lowercase().contains(&q)
                    || a.email.to_lowercase().contains(&q)
                    || a.region.to_lowercase().contains(&q)
                    || a.payment_methods.to_lowercase().contains(&q)
                    || a.notes.to_lowercase().contains(&q)
            })
            .collect()
    };

    let mut list = column![].spacing(10);
    if !has_group {
        list = list.push(text("Select or create a group."));
    } else if accounts.is_empty() {
        list = list.push(text("No accounts yet."));
    } else if filtered.is_empty() {
        list = list.push(text("No accounts match the search."));
    } else {
        for a in filtered {
            list = list.push(account_card(a));
        }
    }

    column![header, search_bar, scrollable(list).height(Length::Fill)]
        .spacing(12)
        .into()
}

fn account_card(a: &Account) -> Element<'_, Message> {
    let title_row = row![
        text(a.site.clone()).size(18),
        horizontal_space(),
        button(text("Edit").size(12)).on_press(Message::EditAccount(a.id)),
        button(text("Delete").size(12)).on_press(Message::DeleteAccount(a.id)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let fields = column![
        field_row("Email", &a.email),
        field_row("Region", &a.region),
        field_row("Payment", &a.payment_methods),
        field_row("Notes", &a.notes),
    ]
    .spacing(2);

    container(column![title_row, fields].spacing(6))
        .padding(10)
        .style(container::rounded_box)
        .into()
}

fn field_row(label: &str, value: &str) -> Element<'static, Message> {
    row![
        text(format!("{label}:"))
            .width(Length::Fixed(90.0))
            .size(13),
        text(value.to_string()).size(13),
    ]
    .spacing(6)
    .into()
}

fn editor_view(e: &AccountEditor) -> Element<'_, Message> {
    let title = if e.id == 0 { "New Account" } else { "Edit Account" };
    column![
        text(title).size(22),
        text_input("Site (e.g. Netflix)", &e.site)
            .on_input(Message::EditSite)
            .padding(8),
        text_input("Email", &e.email)
            .on_input(Message::EditEmail)
            .padding(8),
        text_input("Region (e.g. US, JP)", &e.region)
            .on_input(Message::EditRegion)
            .padding(8),
        text_input("Payment methods", &e.payment_methods)
            .on_input(Message::EditPayment)
            .padding(8),
        text_input("Notes", &e.notes)
            .on_input(Message::EditNotes)
            .padding(8),
        row![
            button(text("Save")).on_press(Message::EditSave),
            button(text("Cancel")).on_press(Message::EditCancel),
        ]
        .spacing(8),
    ]
    .spacing(10)
    .max_width(560)
    .into()
}
