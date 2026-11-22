<div align="center">

# PlicZip

**Архиватор с собственным форматом `.plz`** — индекс в конце файла, BLAKE3 на каждый файл и блок, шифрование паролем, многотомные архивы, безопасная распаковка. Написан на Rust, без C-зависимостей.

</div>

---

## 🌍 Выберите язык / Choose your language

| | Язык | Language | Документация |
|---|---|---|---|
| 🇷🇺 | **[Русский](gitlocales/RU-README.md)** | Russian | Полная документация |
| 🇬🇧 | **[English](gitlocales/EN-README.md)** | English | Full documentation |
| 🇪🇸 | **[Español](gitlocales/ES-README.md)** | Spanish | Documentación completa |
| 🇩🇪 | **[Deutsch](gitlocales/DE-README.md)** | German | Vollständige Dokumentation |
| 🇫🇷 | **[Français](gitlocales/FR-README.md)** | French | Documentation complète |
| 🇨🇳 | **[中文](gitlocales/ZH-README.md)** | Chinese | 完整文档 |

---

## Быстрый старт / Quick start

В Windows для запуска графического интерфейса откройте `PlicZip.exe`. Файл
`pliczip-cli.exe` предназначен для командной строки; `pliczip.exe` сохранён
для совместимости со старыми пакетами и также является CLI-файлом.

```text
$ pliczip a backup.plz ./документы -x9
$ pliczip a секрет.plz ./документы --password=пароль --volume-size 700M
$ pliczip a важное.plz ./документы --recovery 5%
$ pliczip l backup.plz -l
$ pliczip t backup.plz
$ pliczip x backup.plz -o ./восстановлено
```

> Полная документация — в файлах по ссылкам выше. Начните с [Русский](gitlocales/RU-README.md) или [English](gitlocales/EN-README.md).

---

**Лицензия:** MIT OR Apache-2.0

## v1.26.103 — UX 2.0: Operation Center и Recent Archives

### Русский

- Расширен Operation Center: подробные результаты операций и сохранение текста ошибок в истории.
- Operation Center автоматически открывается после ошибки, его можно закрыть клавишей `Esc` или горячей клавишей `Ctrl+Shift+O`.
- Добавлена горячая клавиша `Ctrl+Shift+C` для очистки истории операций.
- В Recent Archives добавлены копирование полного пути, открытие расположения архива и удаление записи.
- Добавлена локализация новых действий, уведомлений, скорости, ETA и единиц времени для RU/EN.
- Исправлены CI-блокеры: удалены неиспользуемые импорты и обновлён `url` до версии `2.5.9`.

### English

- Expanded Operation Center with detailed operation results and persisted error messages in history.
- Operation Center now opens automatically after failures and can be closed with `Esc` or `Ctrl+Shift+O`.
- Added `Ctrl+Shift+C` to clear operation history.
- Recent Archives now supports copying the full path, opening the archive location, and removing an entry.
- Added RU/EN localization for new actions, notifications, speed, ETA, and time units.
- Fixed CI blockers by removing unused imports and updating `url` to `2.5.9`.

## v1.26.102 — UX 2.0: Operation Center

### Русский

- Добавлен домашний экран с drag-and-drop зоной, быстрыми действиями и историей последних архивов.
- Добавлено сохранение последних 8 успешно открытых архивов с повторным открытием одним кликом.
- Добавлен Operation Center с живым прогрессом, процентом выполнения, скоростью и ETA.
- Добавлена история последних завершённых операций: создание, распаковка, добавление файлов, проверка, восстановление и обновление.
- Progress больше не блокирует основное окно модальным диалогом.
- Обновлена локализация RU/EN и синхронизированы версии GUI, CLI и InstallRS Setup.

### English

- Added a Home screen with a drag-and-drop zone, quick actions, and recent archives.
- Added persistent history for the last 8 successfully opened archives with one-click reopening.
- Added an Operation Center with live progress, completion percentage, speed, and ETA.
- Added recent operation history for creating, extracting, adding files, verifying, repairing, and updating.
- Progress is no longer shown as a blocking modal window.
- Updated RU/EN localization and synchronized GUI, CLI, and InstallRS Setup versions.

## v1.26.100

Большое обновление: GUI поддерживает выбор акцента, контекстное меню файлов (переименовать, удалить, выделить), меню пустого места (создать файл и сортировка) и чтение TAR. Создание архивов по-прежнему выполняется только в формате `.plz`; ZIP, 7z, RAR и TAR доступны для чтения, проверки и распаковки.

## Windows Setup

Для Windows в релизах будет доступен `PlicZip-Setup-vX.Y.Z.exe`, собранный через InstallRS. Установщик ставит GUI `PlicZip.exe` и CLI `pliczip-cli.exe` в профиль текущего пользователя без прав администратора, добавляет ярлык, ассоциацию `.plz` и пункты Windows Explorer для `.plz`, `.zip`, `.7z`, `.rar` и `.tar`. Portable ZIP остаётся доступным отдельно. Настройки пользователя при обновлении не удаляются.
