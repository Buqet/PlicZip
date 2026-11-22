# Бенчмарки больших бинарных файлов

Бенчмарк проекта написан на Rust и измеряет Store, Deflate q4, Brotli q5, Brotli q11 и Density на реальных бинарных файлах размером строго больше 100 MiB. Входные файлы передаются явно и не копируются в репозиторий.

Для каждого файла и метода выполняется заданное число повторов. Измеряются размер `.plz`, время упаковки и распаковки. После распаковки проверяется SHA-256 исходного и восстановленного файла. Результаты сохраняются в `large-binary-results.ron`.

Пример запуска:

```bash
cargo build --release --locked --bin large_binary_benchmark
cargo run --release --locked --bin large_binary_benchmark -- \
  --pliczip target/release/pliczip \
  --input /path/to/binary-a \
  --input /path/to/binary-b \
  --repetitions 3 \
  --output benchmark-results/large-binaries-YYYY-MM-DD
```

Не следует запускать такой benchmark в обычном CI: он требует много дискового места и CPU. Его можно запускать вручную на машине с подходящими реальными бинарниками, сохраняя каталог результатов как artifact.
