[Русский](RU-README.md) · [English](EN-README.md) · **Español** · [Deutsch](DE-README.md) · [Français](FR-README.md) · [中文](ZH-README.md)

# PlicZip

Archivador multiplataforma en Rust con formato propio `.plz`. Incluye compresión `store`/deflate/Brotli, hashes BLAKE3, cifrado ChaCha20-Poly1305 con Argon2id, archivos multivolumen y registros de recuperación Reed–Solomon.

## Uso rápido

```text
pliczip a backup.plz ./documentos
pliczip a backup.plz ./documentos --level 9 --solid
pliczip x backup.plz -o ./restaurado
pliczip l backup.plz --long
pliczip t backup.plz
pliczip i backup.plz
```

Los alias `a`, `x`, `l`, `t` e `i` corresponden a `add`, `extract`, `list`, `test` e `info`.

## Contraseña y keyfile

```text
pliczip a secreto.plz ./documentos -p
pliczip x secreto.plz -o ./salida --password-file password.txt
pliczip keygen mi-clave.key --bytes 32
pliczip a secreto.plz ./documentos --keyfile mi-clave.key
pliczip x secreto.plz -o ./salida --keyfile mi-clave.key
```

`--keyfile` lee bytes binarios, por lo que no depende de la codificación del sistema. El comando `keygen` crea un archivo aleatorio y no sobrescribe uno existente. Guarda la contraseña o el keyfile por separado: PlicZip no puede recuperar una clave perdida.

También se puede usar `PLICZIP_PASSWORD` en scripts. `--password-file` lee la primera línea no vacía finalizada por salto de línea.

## Integridad y reparación

Para crear una copia de recuperación usa, por ejemplo, `--recovery 10%`:

```text
pliczip a backup.plz ./documentos --recovery 10%
pliczip t backup.plz
pliczip repair backup.plz
```

`test` informa de los fragmentos dañados sin modificar el archivo. `repair` reconstruye los fragmentos dañados cuando el registro parity contiene suficientes datos y vuelve a escribir los dos trailers. Si la tabla de hashes o demasiados fragmentos están dañados, el comando se detiene sin afirmar que el archivo está reparado.

## Comprobar actualizaciones

No es necesario revisar GitHub manualmente:

```text
pliczip update
pliczip update --json
```

El comando consulta el último release público de GitHub con un timeout corto. Solo muestra la versión y el enlace; no descarga ni ejecuta archivos automáticamente.

## Windows Explorer

En Windows, la GUI permite registrar la integración del menú contextual para el usuario actual. Después de instalar `pliczip-gui.exe`:

```text
pliczip-gui.exe --install-context-menu
```

Aparecen `Abrir en PlicZip` para archivos `.plz` y `Añadir al archivo PlicZip` para archivos y carpetas. Para quitar la integración:

```text
pliczip-gui.exe --uninstall-context-menu
```

## Compilación y pruebas

```text
cargo fmt --all -- --check
cargo test --workspace
cargo audit
cargo build --workspace --release
```

El proyecto incluye CI para formato, auditoría, tests de todo el workspace y Clippy en Linux, Windows y macOS. El fuzzing usa `cargo-fuzz`:

```text
cargo install cargo-fuzz
cargo fuzz run paths
```

El target `paths` prueba las validaciones de rutas de archivos y enlaces contra entradas arbitrarias.

## GUI

`pliczip-gui` comparte la biblioteca con la CLI y permite abrir, crear, verificar y extraer archivos `.plz`. Se puede abrir un archivo pasando su ruta como argumento o desde Windows Explorer.

## Seguridad

Las rutas del archivo se validan al escribir y al leer para evitar zip-slip, rutas absolutas, ADS y nombres reservados de Windows. La extracción comprueba hashes por archivo y por bloque; los límites de expansión protegen contra bombas de compresión. Los enlaces simbólicos se crean al final y se validan contra el directorio de destino.

## Licencia

MIT OR Apache-2.0.

El target de fuzzing necesita Rust nightly: `rustup toolchain install nightly && RUSTUP_TOOLCHAIN=nightly cargo fuzz run paths -- -max_total_time=60`.
