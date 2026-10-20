//! Прогресс фоновых операций: канал из рабочего потока и остаток времени.

use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

use pliczip::prelude::*;
use pliczip::util::{EtaEstimator, ProgressSink};

/// Пересылает обновления прогресса из рабочего потока в окно.
pub struct ChannelSink {
    tx: Sender<Msg>,
}

impl ChannelSink {
    pub fn new(tx: Sender<Msg>) -> Self {
        ChannelSink { tx }
    }
}

impl ProgressSink for ChannelSink {
    fn update(&self, done: u64, total: u64) {
        // Канал без ограничения: send не блокирует рабочий поток.
        let _ = self.tx.send(Msg::Progress { done, total });
    }
}

/// Итог работы рабочего потока.
pub enum Done {
    /// Распаковка.
    Extract {
        stats: Result<ExtractStats, String>,
    },
    /// Проверка целостности.
    Test {
        report: Result<TestReport, String>,
    },
    /// Создание архива.
    Create {
        path: std::path::PathBuf,
        stats: Result<CreateStats, String>,
    },
}

/// Сообщение рабочего потока.
pub enum Msg {
    /// Сделано `done` из `total` байт.
    Progress { done: u64, total: u64 },
    /// Операция закончилась (успехом или ошибкой — см. `Result` внутри).
    Done(Box<Done>),
}

/// Состояние выполняющейся операции для окна прогресса.
pub struct OpState {
    /// Что именно делаем: «сжатие», «распаковка», «проверка».
    pub label: &'static str,
    /// Общий объём байт.
    pub total: u64,
    /// Сколько сделано.
    pub done: u64,
    pub started: Instant,
    eta: EtaEstimator,
}

impl OpState {
    pub fn new(label: &'static str, total: u64) -> Self {
        OpState {
            label,
            total,
            done: 0,
            started: Instant::now(),
            eta: EtaEstimator::new(),
        }
    }

    pub fn progress(&mut self, done: u64, total: u64) {
        self.done = done;
        if total > 0 {
            self.total = total;
        }
        self.eta.sample(done);
    }

    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.done as f64 / self.total as f64 * 100.0).min(100.0)
        }
    }

    /// Скорость в байтах в секунду; `None`, пока данных мало.
    pub fn speed(&self) -> Option<f64> {
        self.eta.speed()
    }

    /// Остаток времени; `None`, пока прогноз невозможен.
    pub fn eta(&self) -> Option<std::time::Duration> {
        self.eta.eta(self.done, self.total)
    }
}

/// Создаёт канал + готовый к передаче в опции sink.
pub fn channel_with_sink() -> (ChannelSink, Sender<Msg>, Receiver<Msg>) {
    let (tx, rx) = std::sync::mpsc::channel();
    (ChannelSink::new(tx.clone()), tx, rx)
}
