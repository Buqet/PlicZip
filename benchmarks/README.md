# Большие бинарные файлы

`large_binary_benchmark.py` измеряет Store, Deflate q4, Brotli q5, Brotli q11 и Density на реальных бинарных файлах размером **строго больше 100 MiB**. Скрипт не генерирует и не добавляет входные файлы в репозиторий: пути передаются явно через `--input`.

Для каждого входного файла и метода выполняется заданное число повторов. Измеряются размер `.plz`, время упаковки и распаковки, а после каждой распаковки проверяется SHA-256 исходного и восстановленного файла. Результаты записываются в JSON и CSV.

Пример запуска на Linux:

```bash
cargo +stable build --release --locked -p pliczip
python3 benchmarks/large_binary_benchmark.py \
  --pliczip target/release/pliczip \
  --input /usr/lib/chromium/chromium \
  --input /home/ubuntu/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/libLLVM.so.23.1-rust-1.100.0-stable \
  --repetitions 3 \
  --output benchmark-results/large-binaries-YYYY-MM-DD
```

Не следует запускать этот benchmark в обычном CI: он требует много дискового места и CPU. Для CI можно запускать его вручную на машине с подходящими реальными бинарниками, сохраняя путь к результатам как artifact.
