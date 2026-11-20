use eframe::egui;
use pliczip::prelude::*;
use pliczip::{format, source};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Instant;
use zeroize::Zeroizing;

use crate::config::{Config, Language};
use crate::locale::Locale;
use crate::progress::{self, Done, Msg, OpState};
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
    pub recovery: Option<(u32, u64, u64)>,
}

pub struct Loaded {
    pub path: PathBuf,
    pub password: Option<Zeroizing<String>>,
    /// Формат внешнего архива; `None` означает native `.plz`.
    pub external_kind: Option<pliczip::external::Kind>,
    pub meta: Meta,
    pub integrity: Integrity,
    pub filter: String,

    pub selected: Option<usize>,
    /// Порядок строк таблицы с учётом фильтра и сортировки.
    pub view: Vec<usize>,
    /// Ключ, по которому построен `view`: пересчёт только при изменении.
    view_key: Option<(String, Sort, bool, u64)>,
    /// Пути каталогов, раскрытых в древовидном представлении.
    pub expanded: HashSet<String>,
    expanded_generation: u64,
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

/// Состояние проверки обновлений в GUI.
#[derive(Clone, Debug, Default)]
pub enum UpdateState {
    /// Проверка ещё не запускалась.
    #[default]
    Idle,
    /// Идёт запрос к GitHub Releases.
    Checking,
    /// Найдена версия новее текущей.
    Available { latest: String, url: String },
    /// Текущая версия актуальна.
    NotFound,
    /// Проверка завершилась ошибкой.
    Error,
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
        (block.stored_size as u128 * entry.size as u128 / block.size as u128).min(u64::MAX as u128)
            as u64,
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
        let key = (
            self.filter.to_lowercase(),
            sort,
            desc,
            self.expanded_generation,
        );
        if self.view_key.as_ref() == Some(&key) {
            return;
        }
        let index = &self.meta.index;
        let filter = &key.0;
        let flat = self.is_flat(sort);
        let dirs: HashSet<&str> = index
            .entries
            .iter()
            .filter(|entry| entry.kind == Kind::Dir)
            .map(|entry| entry.path.as_str())
            .collect();
        let visible = |entry: &PzEntry| {
            if flat {
                return true;
            }
            let mut parent = entry.path.as_str();
            while let Some((prefix, _)) = parent.rsplit_once('/') {
                if dirs.contains(prefix) && !self.expanded.contains(prefix) {
                    return false;
                }
                parent = prefix;
            }
            true
        };
        let mut view: Vec<usize> = index
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                (filter.is_empty() || e.path.to_lowercase().contains(filter.as_str())) && visible(e)
            })
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
            Sort::Packed => {
                view.sort_by_key(|&i| packed_size(index, &index.entries[i]).unwrap_or(0))
            }
            Sort::Ratio => view.sort_by(|&a, &b| {
                let fa = saved_fraction(index, &index.entries[a]).unwrap_or(-1.0);
                let fb = saved_fraction(index, &index.entries[b]).unwrap_or(-1.0);
                fa.total_cmp(&fb)
            }),
            Sort::Method => {
                view.sort_by(|&a, &b| method(&index.entries[a]).cmp(method(&index.entries[b])))
            }
            Sort::Date => view.sort_by_key(|&i| index.entries[i].mtime),
        }
        if desc {
            view.reverse();
        }
        self.view = view;
        self.view_key = Some(key);
    }

    pub fn toggle_dir(&mut self, path: &str) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_owned());
        }
        self.expanded_generation = self.expanded_generation.wrapping_add(1);
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

pub struct RecoveryDlg {
    pub path: PathBuf,
    pub report: Option<Result<pliczip::recovery::VerifyReport, String>>,
}

pub enum Modal {
    None,
    Create(CreateDlg),
    Extract(ExtractDlg),
    Password(PwDlg),
    Rename {
        index: usize,
        value: String,
    },
    NewFile {
        value: String,
    },
    Recovery(RecoveryDlg),
    Error {
        title: String,
        message: String,
        hint: String,
    },
    About,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    None,
    Create,
    Extract,
    Password,
    Rename,
    NewFile,
    Recovery,
    Error,
    About,
    Settings,
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
    /// Последний заголовок, отправленный платформенному окну.
    window_title: String,
    /// Состояние проверки обновлений.
    pub update: UpdateState,
    /// Локаль, загруженная из встроенного SOML-файла.
    pub locale: Locale,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        startup_archive: Option<PathBuf>,
        startup_sources: Option<PathBuf>,
    ) -> Self {
        let cfg = Config::load();
        let locale = Locale::load(cfg.language);
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
            window_title: "PlicZip".into(),
            update: UpdateState::Idle,
            locale,
        };
        if let Some(path) = startup_archive {
            app.begin_open(path);
        }
        if let Some(source) = startup_sources {
            app.modal = Modal::Create(CreateDlg::from_sources(vec![source]));
        }
        app
    }

    pub fn modal_kind(&self) -> ModalKind {
        match self.modal {
            Modal::None => ModalKind::None,
            Modal::Create(_) => ModalKind::Create,
            Modal::Extract(_) => ModalKind::Extract,
            Modal::Password(_) => ModalKind::Password,
            Modal::Rename { .. } => ModalKind::Rename,
            Modal::NewFile { .. } => ModalKind::NewFile,
            Modal::Recovery(_) => ModalKind::Recovery,
            Modal::Error { .. } => ModalKind::Error,
            Modal::About => ModalKind::About,
            Modal::Settings => ModalKind::Settings,
        }
    }

    /// Меняет язык, сразу загружая соответствующий SOML-файл.
    pub fn set_language(&mut self, language: Language) {
        if self.cfg.language == language {
            return;
        }
        self.cfg.language = language;
        self.cfg.save();
        self.locale = Locale::load(language);
    }

    pub fn begin_open(&mut self, path: PathBuf) {
        if self.op.is_some() {
            return;
        }
        if let Ok(Some(kind)) = pliczip::external::detect(&path) {
            if pliczip::external::list(&path, None).is_ok() {
                self.open_external_now(path, kind, None);
            } else {
                self.modal = Modal::Password(PwDlg {
                    path,
                    password: String::new(),
                    error: None,
                });
            }
            return;
        }
        let encrypted = match archive_encrypted(&path) {
            Ok(v) => v,
            Err(err) => {
                self.modal = Modal::Error {
                    title: self.locale.text("open-error-title"),
                    message: format!("{err:#}"),
                    hint: self.locale.text("open-error-plz-hint"),
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
        if let Ok(Some(kind)) = pliczip::external::detect(&path) {
            self.open_external_now(path, kind, password);
            return;
        }
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
                    recovery: ar.recovery.as_ref().map(|trailer| {
                        (
                            trailer.plan.percent,
                            trailer.plan.data_shards,
                            trailer.plan.parity_shards,
                        )
                    }),
                };
                self.loaded = Some(Loaded {
                    path,
                    password,
                    external_kind: None,
                    meta,
                    integrity: Integrity::Untested,
                    filter: String::new(),
                    selected: None,
                    view: Vec::new(),
                    view_key: None,
                    expanded: HashSet::new(),
                    expanded_generation: 0,
                });
                if let Some(loaded) = &mut self.loaded {
                    loaded.expanded = loaded
                        .meta
                        .index
                        .entries
                        .iter()
                        .filter(|entry| entry.kind == Kind::Dir)
                        .map(|entry| entry.path.clone())
                        .collect();
                }
                self.modal = Modal::None;
                self.last_action = None;
            }
            Err(err) => {
                if let Modal::Password(pw) = &mut self.modal {
                    pw.error = Some(format!("{err:#}"));
                    pw.password.clear();
                } else {
                    self.modal = Modal::Error {
                        title: self.locale.text("open-error-title"),
                        message: format!("{err:#}"),
                        hint: self.locale.text("open-error-password-hint"),
                    };
                }
            }
        }
    }

    fn open_external_now(
        &mut self,
        path: PathBuf,
        kind: pliczip::external::Kind,
        password: Option<Zeroizing<String>>,
    ) {
        let password_bytes = password.as_ref().map(|value| value.as_bytes().to_vec());
        match pliczip::external::list(&path, password_bytes.as_deref()) {
            Ok((_, entries)) => {
                let index = pliczip::format::PzIndex {
                    tool: format!("external:{}", kind.name()),
                    entries: entries
                        .iter()
                        .map(|entry| pliczip::format::PzEntry {
                            path: entry.path.clone(),
                            kind: if entry.is_dir {
                                pliczip::format::Kind::Dir
                            } else {
                                pliczip::format::Kind::File
                            },
                            size: entry.size,
                            hash: [0; 32],
                            mode: 0,
                            mtime: 0,
                            block: None,
                            block_offset: 0,
                            link: String::new(),
                        })
                        .collect(),
                    ..Default::default()
                };
                self.loaded = Some(Loaded {
                    path: path.clone(),
                    password,
                    external_kind: Some(kind),
                    meta: Meta {
                        index,
                        encrypted: password_bytes.is_some(),
                        volumes: 1,
                        archive_bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                        recovery: None,
                    },
                    integrity: Integrity::Untested,
                    filter: String::new(),
                    selected: None,
                    view: (0..entries.len()).collect(),
                    view_key: None,
                    expanded: entries
                        .iter()
                        .filter(|entry| entry.is_dir)
                        .map(|entry| entry.path.clone())
                        .collect(),
                    expanded_generation: 0,
                });
                self.modal = Modal::None;
                self.last_action = Some(format!(
                    "открыт {}-архив · {} элементов",
                    kind.name(),
                    entries.len()
                ));
            }
            Err(err) => {
                if let Modal::Password(pw) = &mut self.modal {
                    pw.error = Some(format!("{err:#}"));
                    pw.password.clear();
                } else {
                    self.modal = Modal::Error {
                        title: format!("Не удалось открыть {}-архив", kind.name()),
                        message: format!("{err:#}"),
                        hint: self.locale.text("external-open-error-hint"),
                    };
                }
            }
        }
    }

    pub fn spawn_extract(&mut self, output: PathBuf) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        let path = loaded.path.clone();
        let password = loaded.password.clone();
        let external_kind = loaded.external_kind;
        let total = loaded.meta.index.total_size();

        let (sink, tx, rx) = progress::channel_with_sink();
        let opts = ExtractOptions {
            output,
            show_progress: false,
            progress: Some(std::sync::Arc::new(sink)),
            ..Default::default()
        };
        self.rx = Some(rx);
        self.op = Some(OpState::new(self.locale.text("operation-extract"), total));
        self.modal = Modal::None;
        std::thread::spawn(move || {
            let stats = if external_kind.is_some() {
                let key = password.as_ref().map(|value| value.as_bytes().to_vec());
                match pliczip::external::extract(
                    &path,
                    &opts.output,
                    key.as_deref(),
                    opts.skip_existing,
                ) {
                    Ok((_, stats)) => Ok(ExtractStats {
                        files: stats.files,
                        dirs: stats.dirs,
                        links: 0,
                        skipped: stats.skipped,
                        bytes: stats.bytes,
                    }),
                    Err(err) => Err(format!("{err:#}")),
                }
            } else {
                match Archive::open_with(&path, password.as_deref().map(String::as_str)) {
                    Ok(mut ar) => ar.extract(&opts).map_err(|e| format!("{e:#}")),
                    Err(err) => Err(format!("{err:#}")),
                }
            };
            let _ = tx.send(Msg::Done(Box::new(Done::Extract { stats })));
        });
    }

    /// Добавляет перетащенные файлы в native-архив через безопасную пересборку.
    /// Перед заменой исходник сохраняется как `.plz.bak`.
    pub fn spawn_add_to_archive(&mut self, sources: Vec<PathBuf>) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        if loaded.external_kind.is_some() || sources.is_empty() {
            return;
        }
        let archive = loaded.path.clone();
        let password = loaded.password.clone();
        let (sink, tx, rx) = progress::channel_with_sink();
        self.rx = Some(rx);
        self.op = Some(OpState::new(self.locale.text("operation-add"), 0));
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<CreateStats> {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos();
                let temp = std::env::temp_dir().join(format!("pliczip-add-{stamp}"));
                std::fs::create_dir_all(&temp)?;
                let result = (|| -> anyhow::Result<CreateStats> {
                    let mut ar =
                        Archive::open_with(&archive, password.as_deref().map(String::as_str))?;
                    ar.extract(&ExtractOptions {
                        output: temp.clone(),
                        show_progress: false,
                        ..Default::default()
                    })?;
                    for source in &sources {
                        let name = source
                            .file_name()
                            .ok_or_else(|| anyhow::anyhow!("could not determine file name"))?;
                        copy_path(source, &temp.join(name))?;
                    }
                    let inputs: Vec<PathBuf> = std::fs::read_dir(&temp)?
                        .filter_map(Result::ok)
                        .map(|entry| entry.path())
                        .collect();
                    let replacement = temp.with_extension("plz.tmp");
                    let opts = CreateOptions {
                        password,
                        show_progress: false,
                        progress: Some(std::sync::Arc::new(sink)),
                        ..Default::default()
                    };
                    let stats = pliczip::writer::create(&replacement, &inputs, &opts)?;
                    let backup = archive.with_extension("plz.bak");
                    let _ = std::fs::remove_file(&backup);
                    std::fs::rename(&archive, &backup)?;
                    if let Err(err) = std::fs::rename(&replacement, &archive) {
                        let _ = std::fs::rename(&backup, &archive);
                        return Err(err.into());
                    }
                    let _ = std::fs::remove_file(backup);
                    Ok(stats)
                })();
                let _ = std::fs::remove_dir_all(&temp);
                result
            })();
            let _ = tx.send(Msg::Done(Box::new(Done::Create {
                path: archive,
                stats: result.map_err(|err| format!("{err:#}")),
            })));
        });
    }

    pub fn spawn_rebuild_entry(
        &mut self,
        index: Option<usize>,
        rename_to: Option<String>,
        new_file: Option<String>,
    ) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        if loaded.external_kind.is_some() {
            return;
        }
        let archive = loaded.path.clone();
        let password = loaded.password.clone();
        let old_path = index
            .and_then(|i| loaded.meta.index.entries.get(i))
            .map(|e| e.path.clone());
        let (sink, tx, rx) = progress::channel_with_sink();
        self.rx = Some(rx);
        self.op = Some(OpState::new(self.locale.text("operation-add"), 0));
        std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<CreateStats> {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_nanos();
                let temp = std::env::temp_dir().join(format!("pliczip-edit-{stamp}"));
                std::fs::create_dir_all(&temp)?;
                let result = (|| -> anyhow::Result<CreateStats> {
                    let mut ar =
                        Archive::open_with(&archive, password.as_deref().map(String::as_str))?;
                    ar.extract(&ExtractOptions {
                        output: temp.clone(),
                        show_progress: false,
                        ..Default::default()
                    })?;
                    if let Some(old) = old_path.as_deref() {
                        let source = pliczip::safety::join_safe(&temp, old)?;
                        if let Some(new_name) = rename_to.as_deref() {
                            let target = pliczip::safety::join_safe(&temp, new_name)?;
                            if let Some(parent) = target.parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            std::fs::rename(source, target)?;
                        } else if source.is_dir() {
                            std::fs::remove_dir_all(source)?;
                        } else {
                            std::fs::remove_file(source)?;
                        }
                    }
                    if let Some(name) = new_file.as_deref() {
                        let target = pliczip::safety::join_safe(&temp, name)?;
                        if let Some(parent) = target.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::File::create(target)?;
                    }
                    let inputs: Vec<PathBuf> = std::fs::read_dir(&temp)?
                        .filter_map(Result::ok)
                        .map(|e| e.path())
                        .collect();
                    let replacement = temp.with_extension("plz.tmp");
                    let opts = CreateOptions {
                        password,
                        show_progress: false,
                        progress: Some(std::sync::Arc::new(sink)),
                        ..Default::default()
                    };
                    let stats = pliczip::writer::create(&replacement, &inputs, &opts)?;
                    let backup = archive.with_extension("plz.bak");
                    let _ = std::fs::remove_file(&backup);
                    std::fs::rename(&archive, &backup)?;
                    if let Err(err) = std::fs::rename(&replacement, &archive) {
                        let _ = std::fs::rename(&backup, &archive);
                        return Err(err.into());
                    }
                    let _ = std::fs::remove_file(backup);
                    Ok(stats)
                })();
                let _ = std::fs::remove_dir_all(&temp);
                result
            })();
            let _ = tx.send(Msg::Done(Box::new(Done::Create {
                path: archive,
                stats: result.map_err(|e| format!("{e:#}")),
            })));
        });
    }

    pub fn spawn_test(&mut self) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        let path = loaded.path.clone();
        let password = loaded.password.clone();
        let external_kind = loaded.external_kind;
        let total = loaded.meta.index.total_size();

        let (sink, tx, rx) = progress::channel_with_sink();
        let sink = std::sync::Arc::new(sink);
        self.rx = Some(rx);
        self.op = Some(OpState::new(self.locale.text("operation-test"), total));
        std::thread::spawn(move || {
            let report = if external_kind.is_some() {
                let key = password.as_ref().map(|value| value.as_bytes().to_vec());
                match pliczip::external::test(&path, key.as_deref()) {
                    Ok((_, files)) => Ok(TestReport {
                        blocks: 0,
                        files,
                        bytes: 0,
                    }),
                    Err(err) => Err(format!("{err:#}")),
                }
            } else {
                match Archive::open_with(&path, password.as_deref().map(String::as_str)) {
                    Ok(mut ar) => ar
                        .test(false, 100_000, Some(sink))
                        .map_err(|e| format!("{e:#}")),
                    Err(err) => Err(format!("{err:#}")),
                }
            };
            let _ = tx.send(Msg::Done(Box::new(Done::Test { report })));
        });
    }

    /// Восстанавливает повреждённые куски по recovery record; пароль не нужен.
    pub fn spawn_repair(&mut self) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        let path = loaded.path.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.op = Some(OpState::new(self.locale.text("operation-repair"), 0));
        std::thread::spawn(move || {
            let report = pliczip::recovery::repair_file(&path, false).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::Done(Box::new(Done::Repair { report })));
        });
    }

    pub fn open_recovery_wizard(&mut self) {
        let Some(loaded) = self.loaded.as_ref() else {
            return;
        };
        if loaded.external_kind.is_some() || loaded.meta.recovery.is_none() {
            return;
        }
        self.modal = Modal::Recovery(RecoveryDlg {
            path: loaded.path.clone(),
            report: None,
        });
        self.spawn_recovery_scan();
    }

    pub fn spawn_recovery_scan(&mut self) {
        let Modal::Recovery(dlg) = &self.modal else {
            return;
        };
        let path = dlg.path.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.op = Some(OpState::new(self.locale.text("operation-scan"), 0));
        std::thread::spawn(move || {
            let result = pliczip::source::FileSource::open(&path)
                .map_err(|err| format!("{err:#}"))
                .and_then(|source| {
                    pliczip::recovery::locate(&source)
                        .map_err(|err| format!("{err:#}"))?
                        .ok_or_else(|| "в архиве нет записи для восстановления".to_string())
                })
                .and_then(|trailer| {
                    pliczip::source::FileSource::open(&path)
                        .map_err(|err| format!("{err:#}"))
                        .and_then(|source| {
                            pliczip::recovery::verify(&source, &trailer, false, None)
                                .map_err(|err| format!("{err:#}"))
                        })
                });
            let _ = tx.send(Msg::Done(Box::new(Done::RecoveryScan { report: result })));
        });
    }

    /// Запускает фоновую проверку последнего GitHub Release.
    pub fn spawn_update_check(&mut self) {
        if self.op.is_some() || matches!(self.update, UpdateState::Checking) {
            return;
        }
        self.update = UpdateState::Checking;
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        self.op = Some(OpState::new(self.locale.text("operation-update"), 0));
        std::thread::spawn(move || {
            let result = pliczip::update::check_latest().map_err(|err| format!("{err:#}"));
            let _ = tx.send(Msg::Done(Box::new(Done::Update { result })));
        });
    }

    pub fn spawn_create(&mut self, dlg: &CreateDlg) -> Result<(), String> {
        let output = PathBuf::from(dlg.output.trim());
        if output.as_os_str().is_empty() {
            return Err(self.locale.text("save-path-required"));
        }
        if dlg.sources.is_empty() {
            return Err(self.locale.text("sources-required"));
        }
        let password = if dlg.encrypt {
            if dlg.password.is_empty() {
                return Err(self.locale.text("password-required"));
            }
            if dlg.password != dlg.password2 {
                return Err(self.locale.text("password-mismatch"));
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
            password_bytes: None,
            volume_size,
            show_progress: false,
            progress: Some(std::sync::Arc::new(sink)),
            ..Default::default()
        };
        self.rx = Some(rx);
        self.op = Some(OpState::new(self.locale.text("operation-create"), 0));
        self.modal = Modal::None;
        std::thread::spawn(move || {
            let stats =
                pliczip::writer::create(&output, &inputs, &opts).map_err(|e| format!("{e:#}"));
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
                        title: self.locale.text("operation-interrupted-title"),
                        message: self.locale.text("operation-interrupted-message"),
                        hint: self.locale.text("operation-interrupted-hint"),
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
            .add_filter(
                self.locale.text("archive-filter"),
                &["plz", "zip", "7z", "rar", "tar"],
            )
            .set_title(self.locale.text("open-dialog-title"))
            .pick_file();
        if let Some(path) = picked {
            self.begin_open(path);
        }
    }

    /// Горячие клавиши главного окна.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        if self.modal_kind() != ModalKind::None || self.op.is_some() {
            return;
        }
        let pressed = |key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, key)))
        };
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
                    let count = localized_count(
                        &self.locale,
                        stats.files,
                        "file-one",
                        "file-few",
                        "file-many",
                    );
                    let text = self.locale.format(
                        "extracted",
                        &[&count, &pliczip::util::human_bytes(stats.bytes)],
                    );
                    self.notify(text.clone(), true);
                    self.last_action = Some(text);
                }
                Err(err) => {
                    self.modal = Modal::Error {
                        title: self.locale.text("extract-error-title"),
                        message: err,
                        hint: self.locale.text("extract-error-hint"),
                    };
                }
            },
            Done::Test { report } => match report {
                Ok(report) => {
                    if let Some(loaded) = &mut self.loaded {
                        loaded.integrity = Integrity::Verified(Instant::now());
                    }
                    let count = localized_count(
                        &self.locale,
                        report.files,
                        "file-one",
                        "file-few",
                        "file-many",
                    );
                    let text = self.locale.format("integrity-ok", &[&count]);
                    self.notify(text.clone(), true);
                    self.last_action = Some(text);
                }
                Err(err) => {
                    if let Some(loaded) = &mut self.loaded {
                        loaded.integrity = Integrity::Failed(err.clone());
                    }
                    self.modal = Modal::Error {
                        title: self.locale.text("verify-error-title"),
                        message: err,
                        hint: self.locale.text("verify-error-hint"),
                    };
                }
            },
            Done::Repair { report } => match report {
                Ok(report) => {
                    let text = self.locale.format(
                        "repaired",
                        &[
                            &report.restored_data.to_string(),
                            &report.restored_parity.to_string(),
                        ],
                    );
                    self.notify(text.clone(), true);
                    self.last_action = Some(text);
                    if let Some(loaded) = &mut self.loaded {
                        loaded.integrity = Integrity::Untested;
                    }
                }
                Err(err) => {
                    self.modal = Modal::Error {
                        title: self.locale.text("repair-error-title"),
                        message: err,
                        hint: self.locale.text("repair-error-hint"),
                    };
                }
            },
            Done::RecoveryScan { report } => {
                if let Modal::Recovery(dlg) = &mut self.modal {
                    dlg.report = Some(report);
                }
            }
            Done::Create { path, stats } => match stats {
                Ok(stats) => {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let text = self.locale.format(
                        "created",
                        &[&name, &pliczip::util::human_bytes(stats.archive_bytes)],
                    );
                    self.notify(text.clone(), true);
                    self.begin_open(path);
                    self.last_action = Some(text);
                }
                Err(err) => {
                    self.modal = Modal::Error {
                        title: self.locale.text("create-error-title"),
                        message: err,
                        hint: self.locale.text("create-error-hint"),
                    };
                }
            },
            Done::Update { result } => match result {
                Ok(release) => {
                    let current = release.current.clone();
                    let available =
                        release.latest != current && release.latest != format!("v{current}");
                    if available {
                        self.update = UpdateState::Available {
                            latest: release.latest.clone(),
                            url: release.url,
                        };
                        self.notify(self.locale.format("update-found", &[&release.latest]), true);
                    } else {
                        self.update = UpdateState::NotFound;
                        self.notify(self.locale.format("update-current", &[&current]), true);
                    }
                }
                Err(err) => {
                    self.update = UpdateState::Error;
                    self.modal = Modal::Error {
                        title: self.locale.text("update-error-title"),
                        message: err,
                        hint: self.locale.text("update-error-hint"),
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
        if let Some(archive) = dropped.iter().find(|p| is_archive_path(p)) {
            let archive = archive.clone();
            self.begin_open(archive);
        } else if self.modal_kind() == ModalKind::None {
            if self
                .loaded
                .as_ref()
                .is_some_and(|loaded| loaded.external_kind.is_none())
            {
                self.spawn_add_to_archive(dropped);
            } else {
                self.modal = Modal::Create(CreateDlg::from_sources(dropped));
            }
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
        let title = if let Some(loaded) = &self.loaded {
            let name = loaded
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("{name} — PlicZip")
        } else {
            "PlicZip".into()
        };
        if self.window_title != title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
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
        views::drop_overlay(&ctx, p, &self.locale);
        views::toast(self, &ctx, p);
    }
}

impl CreateDlg {
    pub fn from_sources(sources: Vec<PathBuf>) -> Self {
        let output = sources
            .first()
            .map(|p| {
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "архив".into());
                p.parent()
                    .map(|d| d.join(format!("{stem}.plz")))
                    .unwrap_or_else(|| PathBuf::from(format!("{stem}.plz")))
            })
            .unwrap_or_else(|| PathBuf::from("архив.plz"));
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

fn is_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "plz" | "zip" | "7z" | "rar"
            )
        })
        .unwrap_or(false)
}

fn copy_path(source: &Path, target: &Path) -> anyhow::Result<()> {
    if source.is_dir() {
        std::fs::create_dir_all(target)?;
        for child in std::fs::read_dir(source)? {
            let child = child?;
            copy_path(&child.path(), &target.join(child.file_name()))?;
        }
    } else {
        std::fs::copy(source, target)?;
    }
    Ok(())
}

pub fn plural_ru(n: usize, one: &str, few: &str, many: &str) -> String {
    pliczip::util::plural(n, one, few, many)
}

pub fn localized_count(locale: &Locale, n: usize, one: &str, few: &str, many: &str) -> String {
    let key = if locale.language == "Русский" {
        match n % 10 {
            1 if n % 100 != 11 => one,
            2..=4 if !(12..=14).contains(&(n % 100)) => few,
            _ => many,
        }
    } else if n == 1 {
        one
    } else {
        many
    };
    format!("{n} {}", locale.text(key))
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
