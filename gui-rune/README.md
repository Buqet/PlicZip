# PlicZip Rune Preview

Экспериментальная графическая оболочка PlicZip на Rune. Пакет добавлен параллельно текущему `pliczip-gui` на egui и не заменяет его.

Текущий вертикальный срез содержит демонстрационную таблицу архивных записей, сортировку по колонкам и multi-select через `rune-kit::Table`. Архивные операции и общая модель PlicZip пока не подключены: это следующий этап миграции.

## Запуск

```bash
cargo run -p pliczip-gui-rune
```

Rune является pre-release Git-зависимостью. Версия зафиксирована через `rev` в `Cargo.toml`. В корневом workspace также лежат необходимые vendor-патчи upstream Rune для `parley_engine`, `winit` и `sctk-adwaita`; без них pinned commit не собирается как внешняя зависимость.
