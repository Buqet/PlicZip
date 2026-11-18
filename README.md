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

## v1.26.100

Большое обновление: GUI поддерживает выбор акцента, контекстное меню файлов (переименовать, удалить, выделить), меню пустого места (создать файл и сортировка) и чтение TAR. Создание архивов по-прежнему выполняется только в формате `.plz`; ZIP, 7z, RAR и TAR доступны для чтения, проверки и распаковки.

## Windows Setup

Для Windows в релизах будет доступен `PlicZip-Setup-vX.Y.Z.exe`. Установщик ставит GUI `PlicZip.exe` и CLI `pliczip-cli.exe` в профиль текущего пользователя без прав администратора, добавляет ярлык, ассоциацию `.plz` и пункты Windows Explorer для `.plz`, `.zip`, `.7z`, `.rar` и `.tar`. Portable ZIP остаётся доступным отдельно. Настройки пользователя при обновлении не удаляются.
