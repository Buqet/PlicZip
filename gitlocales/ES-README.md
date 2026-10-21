[馃嚪馃嚭 袪褍褋褋泻懈泄](RU-README.md) 路 [馃嚞馃嚙 English](EN-README.md) 路 [馃嚜馃嚫 Espa帽ol](ES-README.md) 路 [馃嚛馃嚜 Deutsch](DE-README.md) 路 [馃嚝馃嚪 Fran莽ais](FR-README.md) 路 [馃嚚馃嚦 涓枃](ZH-README.md)

---

# PlicZip

Un archivador con su propio formato `.plz`: 铆ndice al final del archivo, compresi贸n deflate/brotli, BLAKE3 en cada archivo y bloque, cifrado con contrase帽a, archivos multivolumen y extracci贸n segura. Escrito en Rust, sin dependencias de C.

```text
$ pliczip a backup.plz ./documentos -x9
$ pliczip a secreto.plz ./documentos --password=secreto --volume-size 700M
$ pliczip l backup.plz -l
$ pliczip t backup.plz
$ pliczip x backup.plz -o ./restaurado
```

> Las rutas en los ejemplos son marcadores de posici贸n 鈥� sustituye tus propios directorios. Si una ruta
> no existe, PlicZip lo dice claramente: `pliczip: error: ruta no encontrada: ".\fotos"`.

## Compilaci贸n

```
cargo build --release
# CLI:  target\release\pliczip.exe (2.7 MB)
# GUI:  target\release\pliczip-gui.exe (14.6 MB)
```

Solo Rust puro: `flate2` (miniz_oxide), `brotli`, `chacha20poly1305`, `argon2` 鈥� sin c贸digo C. Por eso la compilaci贸n no requiere MSVC `cl.exe` y funciona en rutas con caracteres cir铆licos. La GUI usa egui/eframe, tambi茅n sin dependencias de C.

## Comandos

| Comando 鈫曗柧 | Alias 鈫曗柧 | Qu茅 hace 鈫曗柧 |
|---|---|---|
| 鈭抈add <archivo> <rutas...>` | `a` | empaquetar archivos y directorios |
| `extract <archivo>` | `x` | descomprimir |
| `list <archivo>` | `l` | mostrar contenido |
| `test <archivo>` | `t` | verificar integridad sin escribir nada |
| `info <archivo>` | `i` | informaci贸n del archivo |
鈿�

### Empaquetado

```
pliczip a backup.plz ./documentos ./notas     # tus propios directorios, no marcadores
pliczip a -x9 backup.plz ./fuentes            # compresi贸n m谩xima
pliczip a --solid backup.plz ./fuentes        # agrupar archivos peque帽os
pliczip a -x0 backup.plz ./video              # sin compresi贸n, solo contenedor
pliczip a -c "antes de vacaciones" backup.plz .  # con comentario
pliczip a backup.plz src --exclude "**/*.tmp" --exclude "target"
pliczip a src.plz src --include "**/*.rs"     # solo los archivos necesarios
```

Un directorio se guarda con su propio nombre: `a x.plz photos` produce `photos/...` dentro, igual que 7-Zip.

### Niveles de compresi贸n

| Nivel 鈫曗柧 | M茅todo 鈫曗柧 | Cu谩ndo usarlo 鈫曗柧 |
|---|---|---|
| 鈭抈-x0` | sin compresi贸n | ya comprimido (video, jpg, otros archivos) |
| `-x1..4` | deflate | r谩pido, moderado |
| `-x5` (por defecto) | brotli q5 | equilibrio razonable |
| `-x9..11` | brotli q9..q11 | lento, pero texto/c贸digo/logs comprimen notablemente mejor |
鈿�

Si la compresi贸n no reduce un bloque, se guarda tal cual 鈥� el archivo no se infla.

### Filtros

`--exclude` e `--include` aceptan m谩scaras y pueden repetirse:

- `*` 鈥� cualquier car谩cter excepto `/`;
- `?` 鈥� exactamente un car谩cter excepto `/`;
- `**` 鈥� cualquier car谩cter, incluido `/`.

La exclusi贸n siempre prevalece sobre la inclusi贸n. Un directorio excluido ni siquiera se recorre: `--exclude target` en un 谩rbol grande ahorra tiempo, no solo espacio.

### Progreso

El empaquetado, la extracci贸n y la verificaci贸n muestran el progreso en una sola l铆nea de stderr:

```
 compresi贸n 812.4 MiB / 1.42 GiB  57%  186 MiB/s  faltan 4.3s
```

La velocidad y el tiempo restante se calculan con una ventana deslizante de cinco segundos, no con el promedio total: en datos mixtos, el promedio enga帽a enormemente 鈥� los primeros segundos se van en indexaci贸n y archivos peque帽os, luego llegan los gigabytes. `-q` desactiva el progreso.

## Protecci贸n con contrase帽a

```
pliczip a secreto.plz ./documentos -p                  # pide la contrase帽a dos veces
pliczip a secreto.plz ./documentos --password="contrase帽a larga"
pliczip x secreto.plz -o ./out --password="contrase帽a larga"
pliczip t secreto.plz            # pide la contrase帽a de forma interactiva
```

- `-p` sin valor pregunta interactivamente 鈥� la contrase帽a nunca aparece en el
historial del shell ni en la lista de procesos;
- `--password=鈥 solo con signo igual 鈥� de lo contrario `-p backup.plz` se
tragar铆a el nombre del archivo como contrase帽a;
- `--password-file pw.txt` 鈥� la contrase帽a se lee de la primera l铆nea del archivo;
- `PLICZIP_PASSWORD` 鈥� variable de entorno, 煤til para scripts.
- la clave se deriva mediante **Argon2id** (64 MiB, 3 pasadas) de una sal aleatoria;
- los datos y los nombres de archivo se cifran con **ChaCha20-Poly1305** en trozos de 1 MiB;
- el 铆ndice tambi茅n se cifra, as铆 que ni siquiera la lista de archivos se ve sin la contrase帽a;
- una contrase帽a incorrecta y un archivo corrupto son indistinguibles por el mensaje 鈥� no hay or谩culo;
- la contrase帽a no es recuperable. Si la pierdes, pierdes el archivo.

El troceado importa: un archivo de 20 GB se cifra y descifra en flujo, sin ocupar 20 GB de RAM.

## Vol煤menes

```
pliczip a backup.plz ./fotos --volume-size 700M   # produce backup.plz.001, backup.plz.002, 鈥�
pliczip x backup.plz -o ./out      # lee todos los vol煤menes en orden
```

Los vol煤menes son un simple corte de bytes; el formato interno no cambia. El lector no necesita conocer el tama帽o del volumen 鈥� concatena los archivos en orden. Basta con indicar `.001`; el resto se encuentra solo.

Si se pierde un volumen intermedio, el archivo no se descomprime: un error expl铆cito es mejor que un resultado incompleto silencioso.

## Extracci贸n

```
pliczip x backup.plz                     # al directorio actual
pliczip x backup.plz -o ./restaurado
pliczip x backup.plz --skip-existing     # no tocar archivos existentes
pliczip x backup.plz --no-verify         # sin verificaci贸n de hashes (m谩s r谩pido)
```

Por defecto, las sumas de verificaci贸n se comprueban y la extracci贸n se detiene con error si no coinciden. Se restauran las horas de modificaci贸n y los permisos de unix (en Windows, el flag de solo lectura).

## Salida para m谩quinas

`l`, `i` y `t` entienden `--json`:

```
pliczip l backup.plz --json
pliczip i backup.plz --json
pliczip t backup.plz --json     # {"ok": true, ...} o {"ok": false, "error": ...}
```

## Interfaz gr谩fica

```
pliczip-gui                    # ventana vac铆a
pliczip-gui backup.plz         # abrir un archivo (as铆 funciona el doble clic)
```

Ventana en egui/eframe, binario separado en el mismo workspace, comparte la biblioteca con la CLI.

```
鈹屸攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹�
鈹� Archivo  Ver  Ayuda                                       鈹�
鈹溾攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹�
鈹� + A帽adir 鈹� Extraer 鈹� Verificar 鈹� Modificar   [buscar]     鈹�
鈹溾攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹�
鈹� NOMBRE      TAMA脩O EN ARCHIVO M脡TODO  鈥�  鈹� RESUMEN        鈹�
鈹� data/                                 鈥�  鈹� Formato PLZ2   鈹�
鈹�   docs/readme.md        1.6 KiB  28 B brotli 鈹� VOLUMEN     鈹�
鈹�   photo/IMG_0001.jpg    7.3 KiB  38 B brotli 鈹� Original    鈹�
鈹�                                          鈹� Ahorro 99%     鈹�
鈹�                                          鈹� 鈹屸攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹�  鈹�
鈹�                                          鈹� 鈹傗湏 Integridad鈹�  鈹�
鈹�                                          鈹� 鈹� confirmada 鈹�  鈹�
鈹�                                          鈹� 鈹斺攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹�  鈹�
鈹溾攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹粹攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹�
鈹� Listo 路 3 bloques 路 brotli/store 路 normal  8.9 KiB 鈫� 85 B 鈹�
鈹斺攢鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹�
```

Qu茅 puede hacer:

- **apertura** 鈥� desde el men煤, doble clic en `.plz` (ruta como argumento) o
arrastrando al ventana; un archivo cifrado pide contrase帽a;
- **tabla de contenido** 鈥� nombres, tama帽os, proporci贸n en el archivo, m茅todo, fecha;
b煤squeda por nombre;
- **resumen y placa de integridad** 鈥� verde "confirmada", rojo
"corrupta", gris "no comprobada": el estado siempre es visible, no solo bajo demanda;
- **creaci贸n de archivo** 鈥� arrastrar archivos, control "r谩pido / equilibrado /
m谩ximo" con explicaci贸n de consecuencias, solid, contrase帽a, tama帽o de volumen,
comentario;
- **extracci贸n** 鈥� elegir directorio, verificaci贸n de hashes activada por defecto;
- **progreso con pron贸stico honesto** 鈥� velocidad y tiempo restante se
calculan sobre los 煤ltimos segundos, no promediando desde el inicio;
- **ajustes** 鈥� tema (oscuro/claro) y acento (lat贸n/acero/verde)
guardados en `%APPDATA%/pliczip/config.json`.

A煤n no hecho: modificar un archivo existente (bot贸n "Modificar"), asociar `.plz` con el Explorador y el men煤 contextual 鈥� eso es el siguiente bloque.

## Integridad

Cuatro niveles, todos en BLAKE3:

```
archivo 鈹€鈹€hash鈹€鈹€鈹�
                鈹溾攢鈹€ bloque 鈹€鈹€hash鈹€鈹€鈹�
otros archivos 鈹€鈹�                  鈹溾攢鈹€ 铆ndice 鈹€鈹€hash鈹€鈹€鈹�
                                   鈹�                 鈹溾攢鈹€ pie
longitud del 铆ndice 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹�
```

- **archivo** 鈥� detecta corrupci贸n del contenido aunque un bloque se haya descomprimido;
- **bloque** 鈥� detecta corrupci贸n del flujo comprimido completo;
- **铆ndice** 鈥� protege la lista de archivos: sin 茅l, se podr铆an eliminar archivos
del archivo y este seguir铆a pareciendo "v谩lido";
- **firma al inicio y al final** 鈥� detecta un archivo truncado de inmediato, antes de analizarlo.

En los archivos cifrados se a帽ade una etiqueta AEAD a cada trozo, y los hashes de bloque se calculan sobre los datos descifrados 鈥� la verificaci贸n funciona tambi茅n ah铆.

`pliczip t` verifica todo sin crear ni un solo archivo. El c贸digo de salida `1` significa corrupci贸n, `2` 鈥� otros errores.

## Seguridad de la extracci贸n

| Ataque 鈫曗柧 | Defensa 鈫曗柧 |
|---|---|
| 鈭抈../../secret` (zip-slip) | pol铆tica de rutas: `..`, `.`, y segmentos vac铆os prohibidos en el formato |
| rutas absolutas `/etc/鈥, `C:\鈥 | prohibidas en la validaci贸n del 铆ndice |
| ADS en NTFS (`file.txt:hidden`) | el car谩cter `:` est谩 prohibido |
| nombres reservados (`CON`, `NUL`, `aux.txt`) | prohibidos |
| `Report.txt` + `report.txt` | unicidad sin distinguir may煤sculas |
| enlace `docs -> /etc` y escritura a trav茅s de 茅l | los enlaces se crean al final, nunca hacia afuera |
| directorio-enlace plantado previamente | la ruta se comprueba despu茅s de resolver el padre |
| "bomba" de gigabytes | el volumen de escritura se limita por el 铆ndice, m谩s un l铆mite de ratio de compresi贸n |
鈿�

La validaci贸n de rutas se hace dos veces: al escribir (para no crear basura) y al leer (para no descomprimir basura ajena).

## Mediciones

126 MiB de datos mixtos (76 MiB de texto + 50 MiB incompresibles), esta m谩quina, `--release`:

| Modo | Archivo | Del original | Velocidad de empaquetado |
|---|---|---|---|
| `-x1` deflate | 50.7 MiB | 40.2 % | 227 MiB/s |
| `-x5` brotli | 50.0 MiB | 39.6 % | 185 MiB/s |
| `-x9` brotli | 50.0 MiB | 39.6 % | 58 MiB/s |

2000 archivos de 8 KB (33 MiB) 鈥� aqu铆 decide el modo solid:

| Modo 鈫曗柧 | Archivo 鈫曗柧 | Compresi贸n 鈫曗柧 | Tiempo 鈫曗柧 |
|---|---|---|---|
| 鈭抧ormal `-x5` | 0.72 MiB | 45脳 | 1.48 s |
| `--solid -x5` | 0.32 MiB | 101脳 | 0.45 s |
| `--solid -x9` | 0.32 MiB | 103脳 | 1.99 s |
鈿�

La extracci贸n es de unos 700 MiB/s; la verificaci贸n de integridad va a una velocidad similar.

El cifrado a帽ade 16 bytes por megabyte de datos y unos 0.5 s para derivar la clave de la contrase帽a (una vez por archivo).

## Formato

Especificaci贸n completa: [docs/format.md](https://docs/format.md).

```
"PLZ1" | bloques de datos | 铆ndice (CBOR) | longitud del 铆ndice u64 | BLAKE3 del 铆ndice | "PLZ1"
"PLZE" | par谩metros KDF | bloques de datos | 铆ndice | pie            (cifrado)
```

脥ndice al final 鈥� como en 7z: permite escribir el archivo en flujo sin conocer los tama帽os comprimidos de antemano. El costo es que la lectura requiere `seek`, por eso a煤n no se puede leer por tuber铆a.

El lector de la versi贸n 2 abre tambi茅n archivos de la versi贸n 1 鈥� el formato es compatible hacia atr谩s.

## Dise帽o

Identidad e interfaz: [`design/pliczip-design.html`](https://design/pliczip-design.html) 鈥� maqueta autocontenida (marca, paleta, tipograf铆a, CLI, ventana principal, di谩logo de creaci贸n, errores, ideas). Se abre en el navegador; el tema se cambia con un bot贸n arriba a la derecha o con el enlace `#light`.

Decisiones y justificaciones 鈥� en [`docs/design.md`](https://docs/design.md), la marca 鈥� [`design/mark.svg`](https://design/mark.svg), capturas 鈥� `design/preview*.png`.

Principios en breve: el rayo como met谩fora del nombre, el lat贸n como 煤nico acento, el verde y el rojo reservados para la integridad, y una placa permanente de "Integridad confirmada" en la ventana principal 鈥� porque eso es lo 煤nico que distingue esencialmente a PlicZip de 7-Zip.

## Lo que a煤n falta

Una lista honesta, para que no tengas que buscar:

- **recovery record** 鈥� un sector da帽ado no se puede recuperar (en planes; esta es
la diferencia con 7z y zip);
- **deduplicaci贸n** 鈥� los archivos id茅nticos a煤n se guardan por separado, aunque
los hashes ya existen para ello;
- **lectura desde stdin** 鈥� por el 铆ndice al final;
- **nombres no UTF-8** 鈥� una ruta que no se puede representar como UTF-8 no se archiva;
- **modificaci贸n de archivo** 鈥� no se pueden a帽adir ni quitar archivos de un
archivo terminado (el bot贸n "Modificar" en la GUI est谩 vac铆o por ahora);
- **integraci贸n con el Explorador** 鈥� asociaci贸n de `.plz` y men煤 contextual a煤n no;
- **firma digital** del archivo;
- **cifrado solo de nombres** sin cifrar datos 鈥� deliberadamente no se hace: crea
una falsa sensaci贸n de protecci贸n.

## Arquitectura

```
src/
  format.rs    contenedor: firmas, estructuras del 铆ndice, pie, validaci贸n
  crypto.rs    Argon2id, ChaCha20-Poly1305, cifrador y descifrador en flujo
  compress.rs  deflate/brotli, selecci贸n de m茅todo, lector con hashing
  safety.rs    pol铆tica de rutas: normalizaci贸n, zip-slip, nombres de Windows
  glob.rs      m谩scaras --exclude / --include
  source.rs    fuente de bytes: archivo 煤nico o conjunto de vol煤menes
  writer.rs    recorrido del FS, planificaci贸n de bloques, compresi贸n paralela, vol煤menes
  reader.rs    apertura, lista, extracci贸n, verificaci贸n de integridad
  util.rs      tama帽os, progreso con ETA, pluralizaci贸n, UTF-8 en consola de Windows
  main.rs      CLI sobre clap
gui/
  main.rs      punto de entrada, lanzamiento de ventana
  app.rs       estado: archivo abierto, di谩logos, operaciones en segundo plano
  views.rs     ventana principal: men煤, barra de herramientas, tabla, resumen, barra de estado
  dialogs.rs   creaci贸n, extracci贸n, contrase帽a, error, progreso
  theme.rs     paleta de docs/design.md, acentos
  config.rs    ajustes en JSON
  icon.rs      marca de la aplicaci贸n, construida en c贸digo
  progress.rs  canal de progreso y estimaci贸n de tiempo restante
tests/
  cli.rs       pruebas de extremo a extremo mediante procesos reales
  security.rs  archivos maliciosos ensamblados a mano
docs/format.md especificaci贸n
design/        identidad y maqueta de interfaz
legacy/        prototipo del formato YZ01 del que creci贸 el proyecto
```

El progreso pasa por `ProgressSink`: la biblioteca informa, la CLI imprime una l铆nea en stderr, la GUI llena una barra y estima el tiempo restante. Por eso la ventana no se bloquea durante el empaquetado.

La memoria est谩 acotada: los bloques de hasta 32 MiB se ensamblan en memoria y se comprimen en paralelo (rayon); lo m谩s grande se comprime en flujo directamente al archivo. Lo mismo con el cifrado: trozos de 1 MiB. Un archivo de decenas de gigabytes se empaqueta y descomprime sin ocupar la misma cantidad de memoria.

## Desarrollo

```
cargo test --workspace   # 104 pruebas: unitarias, CLI, seguridad, GUI, doc-test
cargo build --release
```

La parte de biblioteca est谩 disponible por separado:

```
use pliczip::prelude::*;

create(&archive, &[input], &CreateOptions::default())?;
let mut ar = Archive::open_with(&archive, Some("contrase帽a"))?;
ar.extract(&ExtractOptions { output: dir, ..Default::default() })?;
```

Licencia: MIT OR Apache-2.0.

