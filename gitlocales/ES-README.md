[🇷🇺 Русский](RU-README.md) · [🇬🇧 English](EN-README.md) · [🇪🇸 Español](ES-README.md) · [🇩🇪 Deutsch](DE-README.md) · [🇫🇷 Français](FR-README.md) · [🇨🇳 中文](ZH-README.md)

---

# PlicZip

Un archivador con su propio formato `.plz`: índice al final del archivo, compresión deflate/brotli, BLAKE3 en cada archivo y bloque, cifrado con contraseña, archivos multivolumen y extracción segura. Escrito en Rust, sin dependencias de C.

```text
$ pliczip a backup.plz ./documentos -x9
$ pliczip a secreto.plz ./documentos --password=secreto --volume-size 700M
$ pliczip l backup.plz -l
$ pliczip t backup.plz
$ pliczip x backup.plz -o ./restaurado
```

> Las rutas en los ejemplos son marcadores de posición — sustituye tus propios directorios. Si una ruta
> no existe, PlicZip lo dice claramente: `pliczip: error: ruta no encontrada: ".\fotos"`.

## Compilación

```
cargo build --release
# CLI:  target\release\pliczip.exe (2.7 MB)
# GUI:  target\release\pliczip-gui.exe (14.6 MB)
```

Solo Rust puro: `flate2` (miniz_oxide), `brotli`, `chacha20poly1305`, `argon2` — sin código C. Por eso la compilación no requiere MSVC `cl.exe` y funciona en rutas con caracteres cirílicos. La GUI usa egui/eframe, también sin dependencias de C.

## Comandos

| Comando | Alias | Qué hace |
|---|---|---|
| `add <archivo> <rutas...>` | `a` | empaquetar archivos y directorios |
| `extract <archivo>` | `x` | descomprimir |
| `list <archivo>` | `l` | mostrar contenido |
| `test <archivo>` | `t` | verificar integridad sin escribir nada |
| `info <archivo>` | `i` | información del archivo |

### Empaquetado

```
pliczip a backup.plz ./documentos ./notas     # tus propios directorios, no marcadores
pliczip a -x9 backup.plz ./fuentes            # compresión máxima
pliczip a --solid backup.plz ./fuentes        # agrupar archivos pequeños
pliczip a -x0 backup.plz ./video              # sin compresión, solo contenedor
pliczip a -c "antes de vacaciones" backup.plz .  # con comentario
pliczip a backup.plz src --exclude "**/*.tmp" --exclude "target"
pliczip a src.plz src --include "**/*.rs"     # solo los archivos necesarios
```

Un directorio se guarda con su propio nombre: `a x.plz photos` produce `photos/...` dentro, igual que 7-Zip.

### Niveles de compresión

| Nivel | Método | Cuándo usarlo |
|---|---|---|
| `-x0` | sin compresión | ya comprimido (video, jpg, otros archivos) |
| `-x1..4` | deflate | rápido, moderado |
| `-x5` (por defecto) | brotli q5 | equilibrio razonable |
| `-x9..11` | brotli q9..q11 | lento, pero texto/código/logs comprimen notablemente mejor |

Si la compresión no reduce un bloque, se guarda tal cual — el archivo no se infla.

### Filtros

`--exclude` e `--include` aceptan máscaras y pueden repetirse:

- `*` — cualquier carácter excepto `/`;
- `?` — exactamente un carácter excepto `/`;
- `**` — cualquier carácter, incluido `/`.

La exclusión siempre prevalece sobre la inclusión. Un directorio excluido ni siquiera se recorre: `--exclude target` en un árbol grande ahorra tiempo, no solo espacio.

### Progreso

El empaquetado, la extracción y la verificación muestran el progreso en una sola línea de stderr:

```
 compresión 812.4 MiB / 1.42 GiB  57%  186 MiB/s  faltan 4.3s
```

La velocidad y el tiempo restante se calculan con una ventana deslizante de cinco segundos, no con el promedio total: en datos mixtos, el promedio engaña enormemente — los primeros segundos se van en indexación y archivos pequeños, luego llegan los gigabytes. `-q` desactiva el progreso.

## Protección con contraseña

```
pliczip a secreto.plz ./documentos -p                  # pide la contraseña dos veces
pliczip a secreto.plz ./documentos --password="contraseña larga"
pliczip x secreto.plz -o ./out --password="contraseña larga"
pliczip t secreto.plz            # pide la contraseña de forma interactiva
```

- `-p` sin valor pregunta interactivamente — la contraseña nunca aparece en el
historial del shell ni en la lista de procesos;
- `--password=…` solo con signo igual — de lo contrario `-p backup.plz` se
tragaría el nombre del archivo como contraseña;
- `--password-file pw.txt` — la contraseña se lee de la primera línea del archivo;
- `PLICZIP_PASSWORD` — variable de entorno, útil para scripts.
- la clave se deriva mediante **Argon2id** (64 MiB, 3 pasadas) de una sal aleatoria;
- los datos y los nombres de archivo se cifran con **ChaCha20-Poly1305** en trozos de 1 MiB;
- el índice también se cifra, así que ni siquiera la lista de archivos se ve sin la contraseña;
- una contraseña incorrecta y un archivo corrupto son indistinguibles por el mensaje — no hay oráculo;
- la contraseña no es recuperable. Si la pierdes, pierdes el archivo.

El troceado importa: un archivo de 20 GB se cifra y descifra en flujo, sin ocupar 20 GB de RAM.

## Volúmenes

```
pliczip a backup.plz ./fotos --volume-size 700M   # produce backup.plz.001, backup.plz.002, …
pliczip x backup.plz -o ./out      # lee todos los volúmenes en orden
```

Los volúmenes son un simple corte de bytes; el formato interno no cambia. El lector no necesita conocer el tamaño del volumen — concatena los archivos en orden. Basta con indicar `.001`; el resto se encuentra solo.

Si se pierde un volumen intermedio, el archivo no se descomprime: un error explícito es mejor que un resultado incompleto silencioso.

## Extracción

```
pliczip x backup.plz                     # al directorio actual
pliczip x backup.plz -o ./restaurado
pliczip x backup.plz --skip-existing     # no tocar archivos existentes
pliczip x backup.plz --no-verify         # sin verificación de hashes (más rápido)
```

Por defecto, las sumas de verificación se comprueban y la extracción se detiene con error si no coinciden. Se restauran las horas de modificación y los permisos de unix (en Windows, el flag de solo lectura).

## Salida para máquinas

`l`, `i` y `t` entienden `--json`:

```
pliczip l backup.plz --json
pliczip i backup.plz --json
pliczip t backup.plz --json     # {"ok": true, ...} o {"ok": false, "error": ...}
```

## Interfaz gráfica

```
pliczip-gui                    # ventana vacía
pliczip-gui backup.plz         # abrir un archivo (así funciona el doble clic)
```

Ventana en egui/eframe, binario separado en el mismo workspace, comparte la biblioteca con la CLI.

```
┌───────────────────────────────────────────────────────────┐
│ Archivo  Ver  Ayuda                                       │
├───────────────────────────────────────────────────────────┤
│ + Añadir │ Extraer │ Verificar │ Modificar   [buscar]     │
├──────────────────────────────────────────┬────────────────┤
│ NOMBRE      TAMAÑO EN ARCHIVO MÉTODO  …  │ RESUMEN        │
│ data/                                 —  │ Formato PLZ2   │
│   docs/readme.md        1.6 KiB  28 B brotli │ VOLUMEN     │
│   photo/IMG_0001.jpg    7.3 KiB  38 B brotli │ Original    │
│                                          │ Ahorro 99%     │
│                                          │ ┌───────────┐  │
│                                          │ │✓ Integridad│  │
│                                          │ │ confirmada │  │
│                                          │ └───────────┘  │
├──────────────────────────────────────────┴────────────────┤
│ Listo · 3 bloques · brotli/store · normal  8.9 KiB → 85 B │
└───────────────────────────────────────────────────────────┘
```

Qué puede hacer:

- **apertura** — desde el menú, doble clic en `.plz` (ruta como argumento) o
arrastrando al ventana; un archivo cifrado pide contraseña;
- **tabla de contenido** — nombres, tamaños, proporción en el archivo, método, fecha;
búsqueda por nombre;
- **resumen y placa de integridad** — verde "confirmada", rojo
"corrupta", gris "no comprobada": el estado siempre es visible, no solo bajo demanda;
- **creación de archivo** — arrastrar archivos, control "rápido / equilibrado /
máximo" con explicación de consecuencias, solid, contraseña, tamaño de volumen,
comentario;
- **extracción** — elegir directorio, verificación de hashes activada por defecto;
- **progreso con pronóstico honesto** — velocidad y tiempo restante se
calculan sobre los últimos segundos, no promediando desde el inicio;
- **ajustes** — tema (oscuro/claro) y acento (latón/acero/verde)
guardados en `%APPDATA%/pliczip/config.json`.

Aún no hecho: modificar un archivo existente (botón "Modificar"), asociar `.plz` con el Explorador y el menú contextual — eso es el siguiente bloque.

## Integridad

Cuatro niveles, todos en BLAKE3:

```
archivo ──hash──┐
                ├── bloque ──hash──┐
otros archivos ─┘                  ├── índice ──hash──┐
                                   ┘                  ├── pie
longitud del índice ──────────────────────────────────┘
```

- **archivo** — detecta corrupción del contenido aunque un bloque se haya descomprimido;
- **bloque** — detecta corrupción del flujo comprimido completo;
- **índice** — protege la lista de archivos: sin él, se podrían eliminar archivos
del archivo y este seguiría pareciendo "válido";
- **firma al inicio y al final** — detecta un archivo truncado de inmediato, antes de analizarlo.

En los archivos cifrados se añade una etiqueta AEAD a cada trozo, y los hashes de bloque se calculan sobre los datos descifrados — la verificación funciona también ahí.

`pliczip t` verifica todo sin crear ni un solo archivo. El código de salida `1` significa corrupción, `2` — otros errores.

## Seguridad de la extracción

| Ataque | Defensa |
|---|---|
| `../../secret` (zip-slip) | política de rutas: `..`, `.`, y segmentos vacíos prohibidos en el formato |
| rutas absolutas `/etc/…`, `C:\…` | prohibidas en la validación del índice |
| ADS en NTFS (`file.txt:hidden`) | el carácter `:` está prohibido |
| nombres reservados (`CON`, `NUL`, `aux.txt`) | prohibidos |
| `Report.txt` + `report.txt` | unicidad sin distinguir mayúsculas |
| enlace `docs -> /etc` y escritura a través de él | los enlaces se crean al final, nunca hacia afuera |
| directorio-enlace plantado previamente | la ruta se comprueba después de resolver el padre |
| "bomba" de gigabytes | el volumen de escritura se limita por el índice, más un límite de ratio de compresión |

La validación de rutas se hace dos veces: al escribir (para no crear basura) y al leer (para no descomprimir basura ajena).

## Mediciones

126 MiB de datos mixtos (76 MiB de texto + 50 MiB incompresibles), esta máquina, `--release`:

| Modo | Archivo | Del original | Velocidad de empaquetado |
|---|---|---|---|
| `-x1` deflate | 50.7 MiB | 40.2 % | 227 MiB/s |
| `-x5` brotli | 50.0 MiB | 39.6 % | 185 MiB/s |
| `-x9` brotli | 50.0 MiB | 39.6 % | 58 MiB/s |

2000 archivos de 8 KB (33 MiB) — aquí decide el modo solid:

| Modo | Archivo | Compresión | Tiempo |
|---|---|---|---|
| normal `-x5` | 0.72 MiB | 45× | 1.48 s |
| `--solid -x5` | 0.32 MiB | 101× | 0.45 s |
| `--solid -x9` | 0.32 MiB | 103× | 1.99 s |

La extracción es de unos 700 MiB/s; la verificación de integridad va a una velocidad similar.

El cifrado añade 16 bytes por megabyte de datos y unos 0.5 s para derivar la clave de la contraseña (una vez por archivo).

## Formato

Disposición byte a byte: documentación del módulo [`src/format.rs`](../src/format.rs).

```
"PLZ1" | bloques de datos | índice (CBOR) | longitud del índice u64 | BLAKE3 del índice | "PLZ1"
"PLZE" | parámetros KDF | bloques de datos | índice | pie            (cifrado)
```

Índice al final — como en 7z: permite escribir el archivo en flujo sin conocer los tamaños comprimidos de antemano. El costo es que la lectura requiere `seek`, por eso aún no se puede leer por tubería.

El lector de la versión 2 abre también archivos de la versión 1 — el formato es compatible hacia atrás.

## Diseño

Principios en breve: el rayo como metáfora del nombre, el latón como único acento, el verde y el rojo reservados para la integridad, y una placa permanente de "Integridad confirmada" en la ventana principal — porque eso es lo único que distingue esencialmente a PlicZip de 7-Zip.

## Lo que aún falta

Una lista honesta, para que no tengas que buscar:

- **recovery record** — un sector dañado no se puede recuperar (en planes; esta es
la diferencia con 7z y zip);
- **deduplicación** — los archivos idénticos aún se guardan por separado, aunque
los hashes ya existen para ello;
- **lectura desde stdin** — por el índice al final;
- **nombres no UTF-8** — una ruta que no se puede representar como UTF-8 no se archiva;
- **modificación de archivo** — no se pueden añadir ni quitar archivos de un
archivo terminado (el botón "Modificar" en la GUI está vacío por ahora);
- **integración con el Explorador** — asociación de `.plz` y menú contextual aún no;
- **firma digital** del archivo;
- **cifrado solo de nombres** sin cifrar datos — deliberadamente no se hace: crea
una falsa sensación de protección.

## Arquitectura

```
src/
  format.rs    contenedor: firmas, estructuras del índice, pie, validación
  crypto.rs    Argon2id, ChaCha20-Poly1305, cifrador y descifrador en flujo
  compress.rs  deflate/brotli, selección de método, lector con hashing
  safety.rs    política de rutas: normalización, zip-slip, nombres de Windows
  glob.rs      máscaras --exclude / --include
  source.rs    fuente de bytes: archivo único o conjunto de volúmenes
  writer.rs    recorrido del FS, planificación de bloques, compresión paralela, volúmenes
  reader.rs    apertura, lista, extracción, verificación de integridad
  util.rs      tamaños, progreso con ETA, pluralización, UTF-8 en consola de Windows
  main.rs      CLI sobre clap
gui/
  main.rs      punto de entrada, lanzamiento de ventana
  app.rs       estado: archivo abierto, diálogos, operaciones en segundo plano
  views.rs     ventana principal: menú, barra de herramientas, tabla, resumen, barra de estado
  dialogs.rs   creación, extracción, contraseña, error, progreso
  theme.rs     paleta, acentos
  config.rs    ajustes en JSON
  icon.rs      marca de la aplicación, construida en código
  progress.rs  canal de progreso y estimación de tiempo restante
tests/
  cli.rs       pruebas de extremo a extremo mediante procesos reales
  security.rs  archivos maliciosos ensamblados a mano
legacy/        prototipo del formato YZ01 del que creció el proyecto
```

El progreso pasa por `ProgressSink`: la biblioteca informa, la CLI imprime una línea en stderr, la GUI llena una barra y estima el tiempo restante. Por eso la ventana no se bloquea durante el empaquetado.

La memoria está acotada: los bloques de hasta 32 MiB se ensamblan en memoria y se comprimen en paralelo (rayon); lo más grande se comprime en flujo directamente al archivo. Lo mismo con el cifrado: trozos de 1 MiB. Un archivo de decenas de gigabytes se empaqueta y descomprime sin ocupar la misma cantidad de memoria.

## Desarrollo

```
cargo test --workspace   # 104 pruebas: unitarias, CLI, seguridad, GUI, doc-test
cargo build --release
```

La parte de biblioteca está disponible por separado:

```
use pliczip::prelude::*;

create(&archive, &[input], &CreateOptions::default())?;
let mut ar = Archive::open_with(&archive, Some("contraseña"))?;
ar.extract(&ExtractOptions { output: dir, ..Default::default() })?;
```

Licencia: [MIT](../LICENSE-MIT) OR [Apache-2.0](../LICENSE-APACHE).

