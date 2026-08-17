# OneDrive CLI (`od-cli`)

[![Rust](https://img.shields.io/badge/rust-stable-brightgreen.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

一个基于 **Rust** 编写的现代化、高性能 **Microsoft OneDrive 命令行与交互式终端工具**。通过 Azure App（Microsoft Graph API）与 OAuth 2.0 设备码授权流程（Device Code Flow），提供安全、快捷的文件上传、下载、浏览与管理体验。

---

## ✨ 核心特性

- 🚀 **双模式支持**：
  - **命令式模式（Imperative CLI）**：支持一键运行单条命令，适合脚本编排、自动化任务与快速调用。
  - **交互式终端（Interactive REPL Shell）**：类 FTP/SFTP 交互终端，记录当前远程路径（`cd`、`pwd`）、命令历史与快捷文件操作。
- 🔐 **Azure App & 设备码授权（Device Code Flow）**：
  - 终端输出验证码并可自动唤起浏览器登录，无需本地监听特定端口。
  - 支持个人 Microsoft 账户以及企业/学校 Office 365 组织账户（多租户支持）。
  - 本地安全持久化 Token，并在过期时自动无感刷新（Refresh Token）。
- 📦 **完整的文件与目录操作**：
  - **目录浏览** (`ls` / `list`)：精美 Unicode 表格、详细信息（`-l`）、递归（`-r`）、JSON 格式输出。
  - **分块大文件上传** (`upload` / `put`)：小文件直接上传，大文件（>4MB）通过 Microsoft Graph Upload Session 自动分块切片并附带实时进度条。
  - **流式文件下载** (`download` / `get`)：支持单文件与整目录递归下载，配有动态进度条。
  - **终端直读** (`cat`)：流式输出远程文本内容到标准输出，可用于 Unix 管道。
  - **目录管理** (`mkdir`)：支持 `-p` 递归级联创建多级目录。
  - **移动/重命名与复制** (`mv`, `cp`)：支持远程移动、重命名及异步复制。
  - **安全删除** (`rm` / `delete`)：快速删除远程文件或文件夹。
  - **容量配额与元数据** (`info`, `quota`)：可视化网盘配额柱状图（已用/剩余/回收站），以及 SHA1/QuickXor 哈希等详细元数据。
  - **全文检索** (`search`)：快速检索网盘内的文件与文件夹。
  - **共享链接** (`share`)：一键生成匿名/组织内的只读或可编辑分享链接。

---

## 🛠️ 安装与构建

### 1. 从源码编译

确保已安装 [Rust & Cargo](https://rustup.rs/)（建议 1.75+）：

```bash
git clone https://github.com/your-username/od-cli.git
cd od-cli
cargo build --release
```

编译产物位于 `target/release/od-cli`（Windows 下为 `od-cli.exe`），可将其加入系统的 `PATH` 环境变量中。

---

## 🔑 认证与配置

### 1. 登录认证

首次使用只需运行登录命令：

```bash
od-cli auth login
```

终端将输出如下提示：
```text
==> Initiating Microsoft Azure Device Login...

=================== Microsoft Sign-In ===================
1. Open the URL in your browser: https://microsoft.com/devicelogin
2. Enter verification code:      XXXXXXXX
=========================================================
```
浏览器会自动打开（或手动访问该链接），输入验证码并授权即可完成登录。

查看当前登录状态：
```bash
od-cli auth status
# 或
od-cli auth whoami
```

登出并清除本地凭证：
```bash
od-cli auth logout
```

### 2. 自定义 Azure App Registration（可选）

默认情况下，`od-cli` 内置了通用公共客户端 ID，可以直接登录使用。如果您希望在自己的 Azure 租户中注册专用应用：

1. 登录 [Azure Portal (应用注册 App registrations)](https://portal.azure.com/#blade/Microsoft_AAD_RegisteredApps/ApplicationsListBlade)。
2. 点击 **新注册 (New registration)**：
   - 名称：`od-cli`
   - 支持的账户类型：*任何组织目录中的账户和个人 Microsoft 账户 (例如 Skype、Xbox)*
   - 重定向 URI：选择 **公共客户端/移动和桌面应用程序 (Public client/native)**，填入 `https://login.microsoftonline.com/common/oauth2/nativeclient`
3. 进入注册的应用，在 **API 权限 (API permissions)** 中添加 **Microsoft Graph** 委托权限：
   - `Files.ReadWrite.All`
   - `offline_access`
   - `User.Read`
4. 在 **身份验证 (Authentication)** -> **高级设置** 中，将 **“允许公共客户端流” (Allow public client flows)** 设置为 **“是 (Yes)”**。
5. 复制应用的 **应用程序(客户端) ID (Application (client) ID)**，在 CLI 中设置：
   ```bash
   od-cli config set client_id <YOUR_CLIENT_ID>
   od-cli config set tenant_id common   # 或您的组织 tenant_id
   ```

---

## 📖 命令式模式使用指南

### 1. 列出文件与目录 (`ls`)
```bash
# 列出根目录
od-cli ls /

# 详细长格式输出（包含 ID 等）
od-cli ls /photos -l

# 递归列出所有子目录
od-cli ls /documents -r

# JSON 格式输出
od-cli ls /photos --json
```

### 2. 上传文件与目录 (`upload` / `put`)
```bash
# 上传单个文件（自动根据大小选择直传或分块切片上传，带进度条）
od-cli upload ./presentation.pptx /documents/presentation.pptx

# 上传并使用相同文件名
od-cli upload ./video.mp4 /videos/

# 递归上传整个本地文件夹
od-cli upload -r ./my_folder /remote_folder
```

### 3. 下载文件与目录 (`download` / `get`)
```bash
# 下载单个文件到当前目录
od-cli download /documents/contract.pdf .

# 下载单个文件并重命名
od-cli download /documents/contract.pdf ./my_contract.pdf

# 递归下载整个远程文件夹
od-cli download -r /remote_folder ./local_folder
```

### 4. 查看文件内容 (`cat`)
```bash
od-cli cat /notes/todo.txt
# 支持管道操作
od-cli cat /logs/app.log | grep "ERROR"
```

### 5. 创建目录 (`mkdir`)
```bash
# 创建目录（-p 支持级联创建父目录）
od-cli mkdir /projects/2026/rust-tool -p
```

### 6. 移动与重命名 (`mv`)
```bash
# 重命名
od-cli mv /documents/old.txt /documents/new.txt

# 移动到新目录
od-cli mv /downloads/photo.jpg /photos/photo.jpg
```

### 7. 复制文件 (`cp`)
```bash
od-cli cp /templates/invoice.xlsx /clients/2026/invoice.xlsx
```

### 8. 删除文件或文件夹 (`rm`)
```bash
od-cli rm /temp/unwanted_file.zip
od-cli rm /temp/old_folder
```

### 9. 查看配额与元数据 (`info` / `quota`)
```bash
# 查看 OneDrive 总容量、已用容量、剩余空间
od-cli quota

# 查看指定文件的详细元数据（MIME、哈希值、Web链接等）
od-cli info /documents/report.pdf
```

### 10. 搜索文件 (`search`)
```bash
od-cli search "财务报表"
```

### 11. 创建分享链接 (`share`)
```bash
# 创建只读匿名分享链接
od-cli share /photos/trip.jpg

# 创建可编辑分享链接
od-cli share /documents/project.docx -t edit
```

---

## 💻 交互式终端模式（REPL Shell）

直接在终端输入 `od-cli` 或 `od-cli shell` 即可进入交互式终端：

```bash
$ od-cli
=========================================================
     Welcome to OneDrive Interactive Shell (od-cli)     
=========================================================
Type help to see available commands, exit or Ctrl+D to exit.

od-cli [user@outlook.com:/]> ls
┌──────┬──────────────────────┬─────────────┬─────────────────────┐
│ Type │ Name                 │        Size │ Modified            │
├──────┼──────────────────────┼─────────────┼─────────────────────┤
│ DIR  │ Documents/           │     4 items │ 2026-08-15 10:20:00 │
│ DIR  │ Photos/              │    12 items │ 2026-08-16 14:15:30 │
│ FILE │ notes.txt            │     1.25 KB │ 2026-08-17 08:00:12 │
└──────┴──────────────────────┴─────────────┴─────────────────────┘
Total: 3 items

od-cli [user@outlook.com:/]> cd Documents
od-cli [user@outlook.com:/Documents]> pwd
/Documents

od-cli [user@outlook.com:/Documents]> upload ./local_report.pdf
[00:00:02] [########################################] 15.20 MB/15.20 MB (7.60 MB/s, 0s) Upload complete
✓ Uploaded 'local_report.pdf' successfully.

od-cli [user@outlook.com:/Documents]> cat ../notes.txt
TODO: Review project proposal.

od-cli [user@outlook.com:/Documents]> quota
=== OneDrive Storage Quota ===
Total Space:         1.00 TB
Used Space:        145.20 GB
Remaining Space:   878.80 GB
Usage:           [====--------------------------] 14.2%

od-cli [user@outlook.com:/Documents]> exit
Goodbye!
```

### 交互终端内置命令速查

| 命令 | 描述 |
| :--- | :--- |
| `ls [-l] [-r] [path]` | 查看当前或指定目录内容 |
| `cd <path>` | 切换远程工作目录（支持相对路径与 `..`） |
| `pwd` | 打印当前工作目录 |
| `mkdir <folder> [-p]` | 创建文件夹 |
| `cat <file>` | 查看远程文件内容 |
| `upload <local> [remote] [-r]` | 上传文件或文件夹 |
| `download <remote> [local] [-r]` | 下载文件或文件夹 |
| `rm <path>` | 删除文件或文件夹 |
| `mv <src> <tgt>` | 移动或重命名文件/文件夹 |
| `cp <src> <tgt>` | 复制文件/文件夹 |
| `info [path]` | 查看文件元数据或网盘信息 |
| `quota` | 查看网盘存储配额 |
| `search <query>` | 全局搜索文件 |
| `share <path>` | 生成分享链接 |
| `whoami` | 显示当前登录用户信息 |
| `clear` | 清屏 |
| `help` | 查看帮助手册 |
| `exit` / `quit` | 退出交互终端 |

---

## ⚙️ 配置文件说明

配置文件存储在系统的标准配置路径下：
- **Windows**: `%APPDATA%\od-cli\config.json`
- **macOS / Linux**: `~/.config/od-cli/config.json`

历史记录文件存储于同一目录下的 `history.txt` 中。

可通过 `od-cli config` 命令快速管理：
```bash
od-cli config show
od-cli config path
od-cli config set chunk_size_mb 20
```

---

## 📄 开源许可证

本项目遵循 [MIT License](LICENSE) 或 [Apache License 2.0](LICENSE)。
