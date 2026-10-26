[🇷🇺 Русский](RU-README.md) · [🇬🇧 English](EN-README.md) · [🇪🇸 Español](ES-README.md) · [🇩🇪 Deutsch](DE-README.md) · [🇫🇷 Français](FR-README.md) · [🇨🇳 中文](ZH-README.md)

---

# PlicZip

一款拥有自有 `.plz` 格式的压缩工具：索引位于文件末尾，采用 deflate/brotli 压缩，对每个文件和每个块使用 BLAKE3，支持密码加密、多卷归档和安全解压。使用 Rust 编写，无 C 依赖。

```text
$ pliczip a backup.plz ./文档 -x9
$ pliczip a secret.plz ./文档 --password=密码 --volume-size 700M
$ pliczip l backup.plz -l
$ pliczip t backup.plz
$ pliczip x backup.plz -o ./恢复
```

> 示例中的路径均为占位符——请替换为你自己的目录。如果路径不存在，PlicZip 会直接说明：
> `pliczip: error: 路径未找到: ".\照片"`。

## 构建

```
cargo build --release
# CLI:  target\release\pliczip.exe (2.7 MB)
# GUI:  target\release\pliczip-gui.exe (14.6 MB)
```

纯 Rust 实现：`flate2` (miniz_oxide)、`brotli`、`chacha20poly1305`、`argon2`——无 C 代码。因此构建不依赖 MSVC `cl.exe`，并且在包含西里尔字母的路径下也能正常工作。GUI 基于 egui/eframe，同样无 C 依赖。

## 命令

| 命令 | 别名 | 功能 |
|---|---|---|
| `add <归档> <路径...>` | `a` | 打包文件和目录 |
| `extract <归档>` | `x` | 解压 |
| `list <归档>` | `l` | 显示内容 |
| `test <归档>` | `t` | 校验完整性，不写入任何内容 |
| `info <归档>` | `i` | 归档信息 |

### 打包

```
pliczip a backup.plz ./文档 ./笔记          # 自己的目录，非占位符
pliczip a -x9 backup.plz ./源码             # 最大压缩
pliczip a --solid backup.plz ./源码         # 合并小文件
pliczip a -x0 backup.plz ./视频             # 不压缩，仅作容器
pliczip a -c "休假前" backup.plz .          # 带注释
pliczip a backup.plz src --exclude "**/*.tmp" --exclude "target"
pliczip a src.plz src --include "**/*.rs"   # 仅需要的文件
```

目录会以自己的名字存入归档：`a x.plz photos` 会在内部产生 `photos/...`，与 7-Zip 相同。

### 压缩级别

| 级别 | 方法 | 使用场景 |
|---|---|---|
| `-x0` | 不压缩 | 已压缩的文件（视频、jpg、其他归档） |
| `-x1..4` | deflate | 快速、中等 |
| `-x5`（默认） | brotli q5 | 合理平衡 |
| `-x9..11` | brotli q9..q11 | 慢，但文本/代码/日志压缩明显更好 |

如果压缩未能缩小某个块，则按原样存储——归档不会膨胀。

### 过滤器

`--exclude` 和 `--include` 接受通配掩码，可重复使用：

- `*`——除 `/` 外的任意字符；
- `?`——除 `/` 外的恰好一个字符；
- `**`——任意字符，包括 `/`。

排除始终优先于包含。被排除的目录完全不会被遍历：在大目录树上使用 `--exclude target` 节省的是时间，而不仅仅是空间。

### 进度

打包、解压和校验都会在 stderr 上以单行显示进度：

```
 压缩 812.4 MiB / 1.42 GiB  57%  186 MiB/s  剩余 4.3s
```

速度和剩余时间基于五秒的滑动窗口计算，而非从开始到现在的平均值：在混合数据上平均值会严重失真——前几秒花在索引和小文件上，之后才是千兆级数据。`-q` 关闭进度显示。

## 密码保护

```
pliczip a secret.plz ./文档 -p                  # 会要求输入两次密码
pliczip a secret.plz ./文档 --password="长密码"
pliczip x secret.plz -o ./out --password="长密码"
pliczip t secret.plz            # 交互式询问密码
```

- 无值的 `-p` 交互式询问——密码不会进入命令历史，也不会出现在进程列表中；
- `--password=…` 仅支持等号形式——否则 `-p backup.plz` 会把归档名当成密码；
- `--password-file pw.txt`——从文件第一行读取密码；
- `PLICZIP_PASSWORD`——环境变量，方便脚本使用。
- 密钥通过 **Argon2id**（64 MiB，3 次迭代）从随机盐派生；
- 数据和文件名以 **ChaCha20-Poly1305** 按 1 MiB 分块加密；
- 索引也被加密，因此没有密码连文件列表都看不到；
- 错误密码和归档损坏在消息上无法区分——不是预言机；
- 密码不可恢复。丢失即丢失归档。

分块很重要：一个 20 GB 的文件以流方式加解密，不占用 20 GB 内存。

## 分卷

```
pliczip a backup.plz ./照片 --volume-size 700M   # 生成 backup.plz.001、backup.plz.002、…
pliczip x backup.plz -o ./out      # 按顺序读取所有分卷
```

分卷就是简单的字节切分，内部格式不变。读取方无需知道分卷大小——按顺序拼接文件即可。指定 `.001` 就够了，其余的会自动找到。

如果丢失中间某个分卷，归档将无法解压：显式报错总好过静默的不完整结果。

## 解压

```
pliczip x backup.plz                     # 解压到当前目录
pliczip x backup.plz -o ./恢复
pliczip x backup.plz --skip-existing     # 不覆盖已存在的文件
pliczip x backup.plz --no-verify         # 不校验哈希（更快）
```

默认会校验校验和，不匹配时解压会以错误停止。会恢复文件修改时间和 unix 权限（Windows 上为只读标志）。

## 机器可读输出

`l`、`i` 和 `t` 支持 `--json`：

```
pliczip l backup.plz --json
pliczip i backup.plz --json
pliczip t backup.plz --json     # {"ok": true, ...} 或 {"ok": false, "error": ...}
```

## 图形界面

```
pliczip-gui                    # 空窗口
pliczip-gui backup.plz         # 打开归档（双击即为此方式）
```

基于 egui/eframe 的窗口，同一 workspace 中的独立二进制，与 CLI 共享库。

```
┌───────────────────────────────────────────────────────────┐
│ 文件  视图  帮助                                          │
├───────────────────────────────────────────────────────────┤
│ + 添加 │ 解压 │ 校验 │ 修改   [搜索]                       │
├──────────────────────────────────────────┬────────────────┤
│ 名称        大小   归档内   方法    …    │ 概要           │
│ data/                                 —  │ 格式   PLZ2    │
│   docs/readme.md        1.6 KiB  28 B brotli │ 卷         │
│   photo/IMG_0001.jpg    7.3 KiB  38 B brotli │ 原始        │
│                                          │ 节省 99%       │
│                                          │ ┌───────────┐  │
│                                          │ │✓ 完整性    │  │
│                                          │ │ 已确认     │  │
│                                          │ └───────────┘  │
├──────────────────────────────────────────┴────────────────┤
│ 就绪 · 3 个块 · brotli/store · 普通      8.9 KiB → 85 B   │
└───────────────────────────────────────────────────────────┘
```

它能做什么：

- **打开**——通过菜单、双击 `.plz`（路径作为参数）或拖放到窗口；加密归档会询问密码；
- **内容表**——名称、大小、在归档中的占比、方法、日期；按名称搜索；
- **概要和完整性徽章**——绿色"已确认"、红色"已损坏"、灰色"未校验"：状态始终可见，而不是仅在命令执行时才出现；
- **创建归档**——拖入文件、"快速/平衡/最大"滑块并说明后果、solid、密码、卷大小、注释；
- **解压**——选择目录，默认校验哈希；
- **带诚实预估的进度**——速度和剩余时间按最近几秒计算，而不是从开始的平均值；
- **设置**——主题（深/浅）和强调色（黄铜/钢/绿色）保存到 `%APPDATA%/pliczip/config.json`。

尚未完成：修改现有归档（"修改"按钮）、将 `.plz` 与资源管理器关联以及右键菜单——这是下一阶段。

## 完整性

四个层级，全部基于 BLAKE3：

```
文件  ──hash──┐
              ├── 块 ──hash──┐
其他文件 ─────┘              ├── 索引 ──hash──┐
                             ┘               ├── 页脚
索引长度 ────────────────────────────────────┘
```

- **文件**——即使块成功解压，也能发现内容损坏；
- **块**——能发现整个压缩流的损坏；
- **索引**——保护文件列表：没有它，文件可被从归档中剔除，归档仍会看起来"有效"；
- **开头和结尾的签名**——在解析前立即发现被截断的文件。

加密归档为每个块添加 AEAD 标签，块哈希基于解密后的数据计算——因此那里的校验同样有效。

`pliczip t` 校验一切而不创建任何文件。返回码 `1` 表示损坏，`2` 表示其他错误。

## 解压安全

| 攻击 | 防御 |
|---|---|
| `../../secret`（zip-slip） | 路径策略：格式禁止 `..`、`.` 和空段 |
| 绝对路径 `/etc/…`、`C:\…` | 在索引校验时禁止 |
| NTFS 备用数据流（`file.txt:hidden`） | 禁止 `:` 字符 |
| 保留名（`CON`、`NUL`、`aux.txt`） | 禁止 |
| `Report.txt` + `report.txt` | 大小写不敏感的唯一性 |
| 符号链接 `docs -> /etc` 并通过它写入 | 链接最后创建，不指向外部 |
| 预埋的目录符号链接 | 解析父目录后再校验路径 |
| 千兆级"炸弹" | 写入量由索引限定，外加压缩比上限 |

路径校验执行两次：写入时（避免生成垃圾）和读取时（避免解出他人的垃圾）。

## 性能测试

126 MiB 混合数据（76 MiB 文本 + 50 MiB 不可压缩数据），本机，`--release`：

| 模式 | 归档 | 相对原始 | 打包速度 |
|---|---|---|---|
| `-x1` deflate | 50.7 MiB | 40.2 % | 227 MiB/s |
| `-x5` brotli | 50.0 MiB | 39.6 % | 185 MiB/s |
| `-x9` brotli | 50.0 MiB | 39.6 % | 58 MiB/s |

2000 个 8 KB 文件（33 MiB）——这里 solid 模式起决定作用：

| 模式 | 归档 | 压缩比 | 时间 |
|---|---|---|---|
| 普通 `-x5` | 0.72 MiB | 45× | 1.48 s |
| `--solid -x5` | 0.32 MiB | 101× | 0.45 s |
| `--solid -x9` | 0.32 MiB | 103× | 1.99 s |

解压约 700 MiB/s，完整性校验大致相同。

加密为每兆字节数据增加 16 字节，密码派生密钥约需 0.5 秒（每个归档一次）。

## 格式

字节级布局：见 [`src/format.rs`](../src/format.rs) 的模块文档。

```
"PLZ1" | 数据块 | 索引 (CBOR) | 索引长度 u64 | 索引的 BLAKE3 | "PLZ1"
"PLZE" | KDF 参数 | 数据块 | 索引 | 页脚            (加密)
```

索引位于末尾——与 7z 一样：允许在事先不知道压缩大小的情况下以流方式写归档。代价是读取需要 `seek`，因此目前还不能从管道读取。

版本 2 的读取器也能打开版本 1 的归档——格式向后兼容。

## 设计

简要原则：闪电作为名称的隐喻，黄铜作为唯一强调色，绿色和红色保留给完整性，主窗口中常驻"完整性已确认"徽章——因为这是 PlicZip 本质上区别于 7-Zip 的唯一一点。

## 尚未具备

诚实清单，省得你去寻找：

- **recovery record**——损坏的扇区无法恢复（在计划中，这是与 7z 和 zip 的区别）；
- **去重**——相同文件仍单独存储，尽管已有可用的哈希；
- **从 stdin 读取**——由于索引在末尾；
- **非 UTF-8 名称**——无法表示为 UTF-8 的路径不会被归档；
- **归档修改**——无法向已完成的归档添加或从中删除文件（GUI 中的"修改"按钮目前为空）；
- **资源管理器集成**——`.plz` 关联和右键菜单尚不存在；
- **归档数字签名**；
- **仅加密名称**而不加密数据——有意不做：这会制造虚假的安全感。

## 架构

```
src/
  format.rs    容器：签名、索引结构、页脚、校验
  crypto.rs    Argon2id、ChaCha20-Poly1305、流式加密器和解密器
  compress.rs  deflate/brotli、方法选择、哈希读取器
  safety.rs    路径策略：归一化、zip-slip、Windows 名称
  glob.rs      --exclude / --include 掩码
  source.rs    字节来源：单文件或多卷集合
  writer.rs    文件系统遍历、块规划、并行压缩、分卷
  reader.rs    打开、列表、解压、完整性校验
  util.rs      尺寸、带 ETA 的进度、复数化、Windows 控制台中的 UTF-8
  main.rs      基于 clap 的 CLI
gui/
  main.rs      入口点、窗口启动
  app.rs       状态：打开的归档、对话框、后台操作
  views.rs     主窗口：菜单、工具栏、表格、概要、状态栏
  dialogs.rs   创建、解压、密码、错误、进度
  theme.rs     调色板、强调色
  config.rs    JSON 中的设置
  icon.rs      应用标志，由代码构建
  progress.rs  进度通道和剩余时间估算
tests/
  cli.rs       通过真实进程的端到端测试
  security.rs  手工构造的恶意归档
legacy/        项目由此生长而来的 YZ01 格式原型
```

进度通过 `ProgressSink` 传递：库报告进度，CLI 在 stderr 上打印一行，GUI 填充进度条并估算剩余时间。因此打包时窗口不会阻塞。

内存受限：最大 32 MiB 的块在内存中组装并并行压缩（rayon），更大的则直接以流方式压缩到文件。加密同理：1 MiB 分块。几十 GB 的归档打包和解压不会占用同样多的内存。

## 开发

```
cargo test --workspace   # 104 个测试：单元、CLI、安全、GUI、文档测试
cargo build --release
```

库部分可单独使用：

```
use pliczip::prelude::*;

create(&archive, &[input], &CreateOptions::default())?;
let mut ar = Archive::open_with(&archive, Some("密码"))?;
ar.extract(&ExtractOptions { output: dir, ..Default::default() })?;
```

许可：[MIT](../LICENSE-MIT) OR [Apache-2.0](../LICENSE-APACHE)。

