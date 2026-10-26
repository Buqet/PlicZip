use eframe::egui;
use pliczip::prelude::*;
use pliczip::{format, source};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Instant;
use zeroize::Zeroizing;

use crate::config::Config;
use crate::progress::{self, Done, Msg, OpState};
use crate::shell::Launch;
use crate::{dialogs, theme, views};

#[derive(Clone)]
pub enum Integrity {
    Untested,
    Verified(Instant),
    Failed(String),
}

pub struct Meta {
    pub index: PzIndex,
    pub encrypted: bool,
    pub volumes: usize,
    pub archive_bytes: u64,
}

pub struct Loaded {
    pub path: PathBuf,
    pub password: Option<Zeroizing<String>>,
    pub meta: Meta,
    pub integrity: Integrity,
    pub filter: String,

    pub selected: Option<usize>,
    /// Порядок строк таблицы с учётом фильтра и сортировки.
    pub view: Vec<usize>,
    /// Ключ, по которому построен `view`: пересчёт только при изменении.
    view_key: Option<(String, Sort, bool)>,
}

/// Колонка, по которой отсортирована таблица.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sort {
    Path,
    Size,
    Packed,
    Ratio,
    Method,
    Date,
}

/// Всплывающее уведомление в углу окна.
pub struct Toast {
    pub text: String,
    pub ok: bool,
    pub at: Instant,
}

/// Сжатый размер элемента: в обычном режиме точный, в solid — доля блока,
/// пропорциональная размеру файла (иначе сумма по файлам не сойдётся с блоком).
pub fn packed_size(index: &PzIndex, entry: &PzEntry) -> Option<u64> {
    if entry.kind == Kind::Dir {
        return None;
    }
    let block = entry.block.and_then(|b| index.blocks.get(b as usize))?;
    if block.size == 0 {
        return None;
    }
    Some(
        (block.stored_size as u128 * entry.size as u128 / block.size as u128)
            .min(u64::MAX as u128) as u64,
    )
}

/// Доля сэкономленного места у элемента (0..1); `None` — нет данных.
pub fn saved_fraction(index: &PzIndex, entry: &PzEntry) -> Option<f32> {
    let packed = packed_size(index, entry)?;
    if entry.size == 0 {
        return None;
    }
    Some((1.0 - packed as f64 / entry.size as f64).clamp(0.0, 1.0) as f32)
}

impl Loaded {
    /// Пересчитывает порядок строк, если фильтр или сортировка изменились.
    pub fn ensure_view(&mut self, sort: Sort, desc: bool) {
        let key = (self.filter.to_lowercase(), sort, desc);
        if self.view_key.as_ref() == Some(&key) {
            return;
        }
        let index = &self.meta.index;
        let filter = &key.0;
        let mut view: Vec<usize> = index
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| filter.is_empty() || e.path.to_lowercase().contains(filter.as_str()))
            .map(|(i, _)| i)
            .collect();
        let method = |e: &PzEntry| {
            e.block
                .and_then(|b| index.blocks.get(b as usize))
                .map(|b| b.method.name())
                .unwrap_or("")
        };
        match sort {
            Sort::Path => view.sort_by(|&a, &b| index.entries[a].path.cmp(&index.entries[b].path)),
            Sort::Size => view.sort_by_key(|&i| index.entries[i].size),
            Sort::Packed => view.sort_by_key(|&i| packed_size(index, &index.entries[i]).unwrap_or(0)),
            Sort::Ratio => view.sort_by(|&a, &b| {
                let fa = saved_fraction(index, &index.entries[a]).unwrap_or(-1.0);
                let fb = saved_fraction(index, &index.entries[b]).unwrap_or(-1.0);
                fa.total_cmp(&fb)
            }),
            Sort::Method => view.sort_by(|&a, &b| method(&index.entries[a]).cmp(method(&index.entries[b]))),
            Sort::Date => view.sort_by_key(|&i| index.entries[i].mtime),
        }
        if desc {
            view.reverse();
        }
        self.view = view;
        self.view_key = Some(key);
    }

    /// Плоский ли список: при фильтре или сортировке не по пути дерево не строится.
    pub fn is_flat(&self, sort: Sort) -> bool {
        !self.filter.is_empty() || sort != Sort::Path
    }
}

pub struct CreateDlg {
    pub sources: Vec<PathBuf>,
    pub output: String,
    pub level: i32,
    pub solid: bool,
    pub encrypt: bool,
    pub password: String,
    pub password2: String,
    pub comment: String,
    pub volume: String,
    pub error: Option<String>,
}

pub struct ExtractDlg {
    pub output: String,
    pub error: Option<String>,
}

pub struct PwDlg {
    pub path: PathBuf,
    pub password: String,
    pub error: Option<String>,
}

pub enum Modal {
    None,
    Create(CreateDlg),
    Extract(ExtractDlg),
    Password(PwDlg),
    Error {
        title: String,
        message: String,
        hint: String,
    },
    About,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    None,
    Create,
    Extract,
    Password,
    Error,
    About,
}

/// Что сделать сразу после открытия архива (пункты контекстного меню).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AfterOpen {
    ExtractHere,
    ExtractTo,
    Test,
}

pub struct App {
    pub cfg: Config,
    pub loaded: Option<Loaded>,
    pub modal: Modal,
    pub op: Option<OpState>,
    rx: Option<Receiver<Msg>>,

    pub last_action: Option<String>,
    pub sort: Sort,
    pub sort_desc: bool,
    pub toast: Option<Toast>,
    /// Поставить фокус в поле поиска на следующем кадре (Ctrl+F).
    pub focus_search: bool,
    /// Отложенное действие для архива, который сейчас открывается
    /// (зашифрованный сначала спрашивает пароль).
    after_open: Option<(PathBuf, AfterOpen)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        let cfg = Config::load();
        theme::apply(&cc.egui_ctx, &cfg);
        let mut app = App {
            cfg,
            loaded: None,
            modal: Modal::None,
            op: None,
            rx: None,
            last_action: None,
            sort: Sort::Path,
            sort_desc: false,
            toast: None,
            focus_search: false,
            after_open: None,
        };
        app.start(launch);
        app
    }

    /// Выполняет действие, с которым запущено окно.
    fn start(&mut self, launch: Launch) {
        match launch {
            Launch::Empty | Launch::Register | Launch::Unregister => {}
            Launch::Open(path) => self.begin_open(path),
            Launch::Add(sources) => self.modal = Modal::Create(CreateDlg::from_sources(sources)),
            Launch::Compress(sources) => {
                let dlg = CreateDlg::from_sources(sources);
                if let Err(err) = self.spawn_create(&dlg) {
                    self.modal = Modal::Error {
                        title: "Не удалось сжать".into(),
                        message: err,
                        hint: "Попробуй «Добавить в архив…» — там можно выбрать параметры.".into(),
                    };
                }
            }
            Launch::ExtractHere(path) => self.open_then(path, AfterOpen::ExtractHere),
            Launch::ExtractTo(path) => self.open_then(path, AfterOpen::ExtractTo),
            Launch::Test(path) => self.open_then(path, AfterOpen::Test),
        }
    }

    /// Открывает архив и выполняет действие, когда он откроется.
    fn open_then(&mut self, path: PathBuf, action: AfterOpen) {
        self.after_open = Some((path.clone(), action));
        self.begin_open(path);
    }

    /// Запускает отложенное действие, если открыт именно тот архив.
    fn run_after_open(&mut self) {
        let Some((path, action)) = self.after_open.take() else { return };
        let Some(loaded) = &self.loaded else { return };
        if loaded.path != path {
            return;
        }
        match action {
            AfterOpen::ExtractHere => {
                let dir = path
                    .parent()
                    .filter(|d| !d.as_os_str().is_empty())
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                self.spawn_extract(dir);
            }
            AfterOpen::ExtractTo => self.spawn_extract(default_extract_dir(&path)),
            AfterOpen::Test => self.spawn_test(),
        }
    }

    /// Встраивает PlicZip в контекстное меню Проводника или убирает его оттуда.
    pub fn toggle_shell_integration(&mut self) {
        let result = match crate::shell::status() {
            crate::shell::Status::On => crate::shell::unregister().map(|()| "PlicZip убран из контекстного меню"),
            _ => crate::shell::register().map(|()| "PlicZip добавлен в контекстное меню Проводника"),
        };
        match result {
            Ok(text) => self.notify(text, true),
            Err(err) => {
                self.modal = Modal::Error {
                    title: "Не удалось изменить интеграцию".into(),
                    message: err,
                    hint: "Записи делаются в HKCU\\Software\\Classes — права администратора не нужны. \
                           Проверь, не блокирует ли реестр антивирус или групповая политика."
                        .into(),
                };
            }
        }
    }

    pub fn modal_kind(&self) -> ModalKind {
        match self.modal {
            Modal::None => ModalKind::None,
            Modal::Create(_) => ModalKind::Create,
            Modal::Extract(_) => ModalKind::Extract,
            Modal::Password(_) => ModalKind::Password,
            Modal::Error { .. } => ModalKind::Error,
            Modal::About => ModalKind::About,
        }
    }

    pub fn begin_open(&mut self, path: PathBuf) {
        if self.op.is_some() {
            return;
        }
        let encrypted = match archive_encrypted(&path) {
            Ok(v) => v,
            Err(err) => {
                self.after_open = None;
                self.modal = Modal::Error {
                    title: "Не удалось открыть архив".into(),
                    message: format!("{err:#}"),
                    hint: "Проверь, что файл — архив PlicZip (.plz) и не повреждён.".into(),
                };
                return;
            }
        };
        if encrypted {
            self.modal = Modal::Password(PwDlg {
                path,
                password: String::new(),
                error: None,
            });
        } else {
            self.open_now(path, None);
        }
    }

    pub fn open_now(&mut self, path: PathBuf, password: Option<Zeroizing<String>>) {
        match Archive::open_with(&path, password.as_deref().map(String::as_str)) {
            Ok(ar) => {
                let meta = Meta {
                    index: ar.index.clone(),
                    encrypted: ar.is_encrypted(),
                    volumes: ar.files().len(),
                    archive_bytes: ar
                        .files()
                        .iter()
                        .filter_map(|p| std::fs::metadata(p).ok())
                        .map(|m| m.len())
                        .sum(),
                };
                self.loaded = Some(Loaded {
                    path,
                    password,
                    meta,
                    integrity: Integrity::Untested,
                    filter: String::new(),
                    selected: None,
                    view: Vec::new(),
                    view_key: None,
                });
                self.modal = Modal::None;
                self.last_action = None;
                self.run_after_open();
            }
            Err(err) => {
                if let Modal::Password(pw) = &mut self.modal {
                    pw.error = Some(format!("{err:#}"));
                    pw.password.clear();
                } else {
                    self.after_open = None;
                    self.modal = Modal::Error {
                        title: "Не удалось открыть архив".into(),
                        message: format!("{err:#}"),
                        hint: "Если архив зашифрован, введи пароль ещё раз.".into(),
                    };
                }
            }
        }
    }


    pub fn spawn_extract(&mut self, output: PathBuf) {
        let Some(loaded) = self.loaded.as_ref() else { return };
        let path = loaded.path.clone();
        let password = loaded.password.clone();
        let total = loaded.meta.index.total_size();

        let (sink, tx, rx) = progress::channel_with_sink();
        let opts = ExtractOptions {
            output,
            show_progress: false,
            progress: Some(std::sync::Arc::new(sink)),
            ..Default::default()
        };
        self.rx = Some(rx);
        self.op = Some(OpState::new("распаковка", total));
        self.modal = Modal::None;
        std::thread::spawn(move || {
            let stats = match Archive::open_with(&path, password.as_deref().map(String::as_str)) {
                Ok(mut ar) => ar.extract(&opts).map_err(|e| format!("{e:#}")),
                Err(err) => Err(format!("{err:#}")),
            };
            let _ = tx.send(Msg::Done(Box::new(Done::Extract { stats })));
        });
    }

    pub fn spawn_test(&mut self) {
        let Some(loaded) = self.loaded.as_ref() else { return };
        let path = loaded.path.clone();
        let password = loaded.password.clone();
        let total = loaded.meta.index.total_size();

        let (sink, tx, rx) = progress::channel_with_sink();
        let sink = std::sync::Arc::new(sink);
        self.rx = Some(rx);
        self.op = Some(OpState::new("проверка", total));
        std::thread::spawn(move || {
            let report = match Archive::open_with(&path, password.as_deref().map(String::as_str)) {
                Ok(mut ar) => ar
                    .test(false, 100_000, Some(sink))
                    .map_err(|e| format!("{e:#}")),
                Err(err) => Err(format!("{err:#}")),
            };
            let _ = tx.send(Msg::Done(Box::new(Done::Test { report })));
        });
    }

    pub fn spawn_create(&mut self, dlg: &CreateDlg) -> Result<(), String> {
        let output = PathBuf::from(dlg.output.trim());
        if output.as_os_str().is_empty() {
            return Err("укажи, куда сохранить архив".into());
        }
        if dlg.sources.is_empty() {
            return Err("добавь файлы или каталоги".into());
        }
        let password = if dlg.encrypt {
            if dlg.password.is_empty() {
                return Err("пароль не может быть пустым".into());
            }
            if dlg.password != dlg.password2 {
                return Err("пароли не совпадают".into());
            }
            Some(Zeroizing::new(dlg.password.clone()))
        } else {
            None
        };
        let volume_size = if dlg.volume.trim().is_empty() {
            None
        } else {
            Some(parse_size(dlg.volume.trim())?)
        };
        let inputs = dlg.sources.clone();
        let level = dlg.level;
        let solid = dlg.solid;
        let comment = dlg.comment.clone();
        let (sink, tx, rx) = progress::channel_with_sink();
        let opts = CreateOptions {
            level,
            solid,
            comment,
            password,
            volume_size,
            show_progress: false,
            progress: Some(std::sync::Arc::new(sink)),
            ..Default::default()
        };
        self.rx = Some(rx);
        self.op = Some(OpState::new("сжатие", 0));
        self.modal = Modal::None;
        std::thread::spawn(move || {
            let stats = pliczip::writer::create(&output, &inputs, &opts).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Done(Box::new(Done::Create {
                path: output,
                stats,
            })));
        });
        Ok(())
    }

    // Опрашивает канал рабочего потока.
    pub fn poll_ops(&mut self) {
        let Some(rx) = &self.rx else { return };
        loop {
            match rx.try_recv() {
                Ok(Msg::Progress { done, total }) => {
                    if let Some(op) = &mut self.op {
                        op.progress(done, total);
                    }
                }
                Ok(Msg::Done(done)) => {
                    self.op = None;
                    self.rx = None;
                    self.finish_op(*done);
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.op = None;
                    self.rx = None;
                    self.modal = Modal::Error {
                        title: "Операция прервана".into(),
                        message: "рабочий поток завершился неожиданно".into(),
                        hint: "Открой архив заново и повтори.".into(),
                    };
                    return;
                }
            }
        }
    }

    /// Показывает уведомление в углу окна.
    pub fn notify(&mut self, text: impl Into<String>, ok: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            ok,
            at: Instant::now(),
        });
    }

    /// Открывает диалог распаковки для текущего архива.
    pub fn open_extract_dialog(&mut self) {
        if self.op.is_some() {
            return;
        }
        if let Some(loaded) = &self.loaded {
            let dir = default_extract_dir(&loaded.path);
            self.modal = Modal::Extract(ExtractDlg {
                output: dir.to_string_lossy().into_owned(),
                error: None,
            });
        }
    }

    /// Открывает архив через системный диалог.
    pub fn pick_and_open(&mut self) {
        if self.op.is_some() {
            return;
        }
        let picked = rfd::FileDialog::new()
            .add_filter("Архивы PlicZip", &["plz"])
            .set_title("Открыть архив")
            .pick_file();
        if let Some(path) = picked {
            self.after_open = None;
            self.begin_open(path);
        }
    }

    /// Горячие клавиши главного окна.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        if self.modal_kind() != ModalKind::None || self.op.is_some() {
            return;
        }
        let pressed = |key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, key)));
        if pressed(Key::O) {
            self.pick_and_open();
        } else if pressed(Key::N) {
            self.modal = Modal::Create(CreateDlg::from_sources(Vec::new()));
        } else if pressed(Key::E) {
            self.open_extract_dialog();
        } else if pressed(Key::T) {
            if self.loaded.is_some() {
                self.spawn_test();
            }
        } else if pressed(Key::F) {
            self.focus_search = true;
        }
    }

    fn finish_op(&mut self, done: Done) {
        match done {
            Done::Extract { stats } => match stats {
                Ok(stats) => {
                    let text = format!(
                        "распаковано: {} · {}",
                        plural_ru(stats.files, "файл", "файла", "файлов"),
                        pliczip::util::human_bytes(stats.bytes)
                    );
                    self.notify(text.clone(), true);
                    self.last_action = Some(text);
                }
                Err(err) => {
                    self.modal = Modal::Error {
                        title: "Распаковка не удалась".into(),
                        message: err,
                        hint: "Часть файлов могла записаться до ошибки — проверь каталог вывода."
                            .into(),
                    };
                }
            },
            Done::Test { report } => match report {
                Ok(report) => {
                    if let Some(loaded) = &mut self.loaded {
                        loaded.integrity = Integrity::Verified(Instant::now());
                    }
                    let text = format!(
                        "целостность подтверждена · {}",
                        plural_ru(report.files, "файл", "файла", "файлов")
                    );
                    self.notify(text.clone(), true);
                    self.last_action = Some(text);
                }
                Err(err) => {
                    if let Some(loaded) = &mut self.loaded {
                        loaded.integrity = Integrity::Failed(err.clone());
                    }
                    self.modal = Modal::Error {
                        title: "Проверка не пройдена".into(),
                        message: err,
                        hint: "Восстанови повреждённый файл из исходника или пересоздай архив \
                               из целой копии. Остальные файлы, скорее всего, в порядке."
                            .into(),
                    };
                }
            },
            Done::Create { path, stats } => match stats {
                Ok(stats) => {
                    let text = format!(
                        "создан {} · {}",
                        path.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        pliczip::util::human_bytes(stats.archive_bytes)
                    );
                    self.notify(text.clone(), true);
                    self.begin_open(path);
                    self.last_action = Some(text);
                }
                Err(err) => {
                    self.modal = Modal::Error {
                        title: "Создание не удалось".into(),
                        message: err,
                        hint: "Проверь права на запись и место на диске.".into(),
                    };
                }
            },
        }
    }

    pub fn handle_dnd(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if dropped.is_empty() || self.op.is_some() {
            return;
        }
        if let Some(plz) = dropped.iter().find(|p| is_plz(p)) {
            let plz = plz.clone();
            self.after_open = None;
            self.begin_open(plz);
        } else if self.modal_kind() == ModalKind::None {
            self.modal = Modal::Create(CreateDlg::from_sources(dropped));
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_ops();
        if self.op.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        theme::apply(&ctx, &self.cfg);
        self.handle_dnd(&ctx);

        let p = theme::current();
        if let Some(loaded) = &self.loaded {
            let name = loaded
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{name} — PlicZip")));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title("PlicZip".into()));
        }
        self.shortcuts(&ctx);
        let busy = self.op.is_some();
        if let Some(loaded) = &mut self.loaded {
            loaded.ensure_view(self.sort, self.sort_desc);
        }

        views::menu_bar(self, ui);
        views::toolbar(self, ui, busy);
        views::status_bar(self, ui);
        if self.loaded.is_some() {
            views::archive_header(self, ui, p);
            views::side_panel(self, ui, p);
        }
        views::central(self, ui, p, busy);

        dialogs::draw(self, &ctx, p);
        views::drop_overlay(&ctx, p);
        views::toast(self, &ctx, p);
    }
}

impl CreateDlg {
    pub fn from_sources(sources: Vec<PathBuf>) -> Self {
        // Рядом с источником, его именем; несколько — именем общего каталога;
        // занятое имя не перезаписывается: «имя (2).plz».
        let output = crate::shell::args::archive_name_for(&sources, Path::is_dir, Path::exists);
        CreateDlg {
            sources,
            output: output.to_string_lossy().into_owned(),
            level: 5,
            solid: false,
            encrypt: false,
            password: String::new(),
            password2: String::new(),
            comment: String::new(),
            volume: String::new(),
            error: None,
        }
    }
}

fn archive_encrypted(path: &Path) -> anyhow::Result<bool> {
    let (src, _) = source::open(path)?;
    let layout = format::inspect(src.as_ref())?;
    Ok(layout.encrypted)
}

/// Каталог распаковки по умолчанию: рядом с архивом, его же именем.
pub fn default_extract_dir(archive: &Path) -> PathBuf {
    let stem = archive
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "распаковано".into());
    archive
        .parent()
        .map(|d| d.join(stem.clone()))
        .unwrap_or_else(|| PathBuf::from(stem))
}

pub fn is_plz(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("plz"))
        .unwrap_or(false)
}

pub fn plural_ru(n: usize, one: &str, few: &str, many: &str) -> String {
    pliczip::util::plural(n, one, few, many)
}

pub fn parse_size(text: &str) -> Result<u64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("пустой размер".to_string());
    }
    let (digits, multiplier) = match trimmed.chars().last().unwrap() {
        'k' | 'K' => (&trimmed[..trimmed.len() - 1], 1024u64),
        'm' | 'M' => (&trimmed[..trimmed.len() - 1], 1024 * 1024),
        'g' | 'G' => (&trimmed[..trimmed.len() - 1], 1024 * 1024 * 1024),
        _ => (trimmed, 1),
    };
    let value: u64 = digits
        .trim()
        .parse()
        .map_err(|_| format!("не понимаю размер {text:?}"))?;
    value
        .checked_mul(multiplier)
        .filter(|v| *v > 0)
        .ok_or_else(|| format!("недопустимый размер {text:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_size("8M").unwrap(), 8 * 1024 * 1024);
        assert_eq!(parse_size("512k").unwrap(), 512 * 1024);
        assert_eq!(parse_size("1G").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size("100").unwrap(), 100);
        assert!(parse_size("0").is_err());
        assert!(parse_size("abc").is_err());
        assert!(parse_size("").is_err());
    }

    #[test]
    fn plural_forms() {
        assert_eq!(plural_ru(1, "файл", "файла", "файлов"), "1 файл");
        assert_eq!(plural_ru(2, "файл", "файла", "файлов"), "2 файла");
        assert_eq!(plural_ru(5, "файл", "файла", "файлов"), "5 файлов");
        assert_eq!(plural_ru(11, "файл", "файла", "файлов"), "11 файлов");
        assert_eq!(plural_ru(21, "файл", "файла", "файлов"), "21 файл");
    }

    #[test]
    fn detects_plz_by_extension() {
        assert!(is_plz(std::path::Path::new("a.plz")));
        assert!(is_plz(std::path::Path::new("A.PLZ")));
        assert!(!is_plz(std::path::Path::new("a.zip")));
        assert!(!is_plz(std::path::Path::new("plz")));
    }
}
