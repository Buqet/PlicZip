<div align="center">

# PlicZip

**Архиватор с собственным форматом `.plz`** — индекс в конце файла, BLAKE3 на каждый файл и блок, шифрование паролем, многотомные архивы, безопасная распаковка. Написан на Rust, без C-зависимостей.

</div>

---

## 🌍 Выберите язык / Choose your language

| | Язык | Language | Документация |
|---|---|---|---|
| 🇷🇺 | **[Русский](RU-README.md)** | Russian | Полная документация |
| 🇬🇧 | **[English](EN-README.md)** | English | Full documentation |
| 🇪🇸 | **[Español](ES-README.md)** | Spanish | Documentación completa |
| 🇩🇪 | **[Deutsch](DE-README.md)** | German | Vollständige Dokumentation |
| 🇫🇷 | **[Français](FR-README.md)** | French | Documentation complète |
| 🇨🇳 | **[中文](ZH-README.md)** | Chinese | 完整文档 |

---

## Быстрый старт / Quick start

```text
$ pliczip a backup.plz ./документы -x9
$ pliczip a секрет.plz ./документы --password=пароль --volume-size 700M
$ pliczip l backup.plz -l
$ pliczip t backup.plz
$ pliczip x backup.plz -o ./восстановлено
```

> Полная документация — в файлах по ссылкам выше. Начните с [Русский](https://ru-readme.md/) или [English](https://en-readme.md/).

---

**Лицензия:** MIT OR Apache-2.0

