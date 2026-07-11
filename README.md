# Codex History Viewer

一个用于查看、搜索和云端同步 [Codex Desktop](https://openai.com/codex) 历史会话的工具集，包含本地桌面客户端和云端同步服务端。

## 项目结构

```
codex-history-viewer/
├── src/               # Tauri 前端（HTML/CSS/JS）
├── src-tauri/         # Tauri 后端（Rust）
└── server/            # 云端同步服务端（Node.js + Fastify + PostgreSQL）
```

---

## 客户端（Tauri 桌面应用）

### 功能特性

- 📋 **会话列表** — 分页浏览全部历史会话，展示标题、时间、所属项目、模型
- 📁 **按项目浏览** — 按工作目录（cwd）分组，快速定位某个项目的所有对话
- 🔍 **三层搜索**
  - **列表过滤**：当前页即时过滤，显示匹配数（如 `5/30`），纯客户端无延迟
  - **会话内搜索**：展开详情后可搜索对话内容，黄色高亮匹配，`↑↓` 导航跳转
  - **全局搜索**：跨所有会话文件（SQLite + JSONL）深度检索
- 💬 **对话详情** — 侧边栏展示完整多轮问答，支持全屏查看，多段回答自动合并
- ☁️ **云端同步** — 增量上传会话到自建服务端，跨设备查看历史
- 📥 **Agy 会话导入** — 将 JSON、JSONL、TXT、Markdown 格式的 Agy 历史写入 Codex 本地历史
- 🖥️ **Headless CLI** — 在服务器、SSH 和纯终端环境中执行 Agy 会话预览与导入
- ⌨️ **键盘快捷键** — `Cmd+F` / `Cmd+G` / `Esc`

### 数据来源

直接读取 Codex Desktop 本地数据，无需任何服务器：

```
~/.codex/
├── state_5.sqlite      # 会话索引（标题、时间、项目、模型）
└── sessions/           # 完整对话内容（JSONL 格式）
```

### 技术栈

- **前端**：HTML + CSS + JavaScript（无框架）
- **后端**：Rust（rusqlite、walkdir）
- **框架**：[Tauri v2](https://tauri.app/)

### 环境要求

- [Rust](https://rustup.rs/) 1.77+（需将 `~/.cargo/bin` 加入 PATH）
- [Node.js](https://nodejs.org/) 18+
- Tauri 系统依赖（见 [官方文档](https://tauri.app/start/prerequisites/)）

### 快速开始

```bash
# 安装依赖
npm install

# 开发模式启动
npm run dev

# 打包发布版本
npm run build
```

> **注意**：若报 `cargo not found`，请先执行：
> ```bash
> echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
> ```

### 发布流程

GitHub Actions 负责构建和发布。维护者只需要推送 `v*` tag：

```bash
git tag -a v1.3.10 -m "v1.3.10"
git push origin main
git push origin v1.3.10
```

Release workflow 会先创建 draft，等待 GUI、CLI、稳定文件名别名和安装脚本全部上传完成后，最后一步自动公开 Release。正常情况下不需要再到 GitHub 页面手动点击 Publish。

### 安装与更新

Release 同时提供桌面 GUI 安装包和纯终端 CLI。GUI 适合本机查看、搜索和同步会话；CLI 适合服务器、SSH、无桌面环境和自动化脚本。

当前示例版本为 `v1.3.8`。如果 Release 仍是 draft 或仓库是 private，需要先执行 `gh auth login`，或设置有仓库读取权限的 `GH_TOKEN`。

#### 桌面应用在线安装

Linux x64 会安装 AppImage 到 `~/.local/bin/codex-history-viewer` 并创建 desktop entry；macOS 会下载 DMG 并复制 `.app` 到 `/Applications`，无权限时改用 `~/Applications`；Windows 会下载并运行安装器。

```bash
# Linux x64 / macOS，安装最新公开 Release
curl -fsSL https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-gui.sh | bash

# 安装指定版本
curl -fsSL https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-gui.sh | VERSION=v1.3.8 bash
```

```powershell
# Windows PowerShell
iwr https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-gui.ps1 -OutFile install-gui.ps1
.\install-gui.ps1 -Version v1.3.8
```

可用 GUI Release 资产：

| 系统 | 稳定文件名 |
|------|------------|
| macOS Apple Silicon | `codex-history-viewer-macOS-arm64.dmg` |
| macOS Intel | `codex-history-viewer-macOS-x64.dmg` |
| Windows x64 | `codex-history-viewer-Windows-x64-setup.exe` / `codex-history-viewer-Windows-x64.msi` |
| Linux x64 | `codex-history-viewer-Linux-x64.AppImage` / `.deb` / `.rpm` |

GUI 的 Linux 安装包目前只发布 x64；Linux ARM64 纯终端环境请使用 `codex-history-cli-Linux-arm64`。

#### 纯终端 CLI 在线安装

CLI 是独立二进制，不启动 Tauri。Windows/macOS 可直接运行；Linux x64 使用 musl 静态构建；所有 CLI 产物都不依赖 GTK/WebKit 等桌面运行库。

```bash
# Linux / macOS，安装最新公开 Release 到 ~/.local/bin
curl -fsSL https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-cli.sh | bash

# 安装指定版本
curl -fsSL https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-cli.sh | VERSION=v1.3.8 bash
```

```powershell
# Windows PowerShell
iwr https://raw.githubusercontent.com/biglone/codex-history-viewer/main/scripts/install-cli.ps1 -OutFile install-cli.ps1
.\install-cli.ps1 -Version v1.3.8 -AddToPath
```

可用 CLI Release 资产：

| 系统 | 稳定文件名 |
|------|------------|
| macOS Apple Silicon | `codex-history-cli-macOS-arm64` |
| macOS Intel | `codex-history-cli-macOS-x64` |
| Windows x64 | `codex-history-cli-Windows-x64.exe` |
| Linux x64 | `codex-history-cli-Linux-x64` |
| Linux ARM64 | `codex-history-cli-Linux-arm64`，面向常见 glibc 发行版 |

#### CLI 导入 Agy 会话

先预览，不写入 Codex：

```bash
codex-history-cli agy-import preview --source ~/.agy --json
```

确认候选会话无误后正式导入：

```bash
codex-history-cli agy-import run --source ~/.agy
```

如果 Agy 历史不在 `~/.agy`，把 `--source` 换成实际目录或导出的 `.json/.jsonl/.ndjson/.txt/.md` 文件。

导入会写入：

```text
~/.codex/state_5.sqlite
~/.codex/agy_imported/<session-id>.jsonl
```

如果设置了 `CODEX_HOME` 或 `CODEX_SQLITE_HOME`，CLI 会按 Codex 的环境变量读取对应目录。重复导入同一版本时会返回 `skipped`，失败时进程退出码为非零。

#### 从源码构建 CLI

```bash
cargo build --manifest-path cli/Cargo.toml --release
./cli/target/release/codex-history-cli --help
```

### 键盘快捷键

| 快捷键 | 说明 |
|--------|------|
| `Cmd+F` | 列表页：聚焦过滤栏；详情面板开着：聚焦会话内搜索 |
| `Cmd+G` | 打开全局搜索 |
| `Enter` | 跳到下一处搜索匹配 |
| `Shift+Enter` | 跳到上一处搜索匹配 |
| `Esc` | 退出全屏 / 关闭搜索 / 关闭详情面板 |

---

## 服务端（云端同步）

### 功能特性

- 🔄 **增量同步** — 仅上传本地比服务端更新的会话，避免重复传输
- 📦 **批量上传** — 支持最多 200 条/请求的批量上传
- 🔎 **远程搜索** — 在服务端对所有设备的会话进行关键词搜索
- 🔐 **Bearer Token 鉴权** — 简单可靠的个人使用鉴权方案
- 🐳 **Docker 一键启动** — PostgreSQL + Adminer 一行命令就绪

### 技术栈

- **运行时**：Node.js 18+
- **框架**：Fastify v4
- **数据库**：PostgreSQL 16（Docker）
- **存储**：本地文件系统（JSONL 消息文件）

### 快速开始

```bash
cd server

# 1. 配置环境变量
cp .env.example .env
# 编辑 .env，修改 API_TOKEN 为随机字符串：
# openssl rand -hex 32

# 2. 启动 PostgreSQL（需要 Docker）
docker-compose up -d

# 3. 初始化数据库表
npm install
node src/db/init.js

# 4. 启动服务（开发模式，:3456 端口）
npm run dev
```

### API 接口

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/sync/health` | 健康检查（无需鉴权） |
| `GET` | `/api/sync/stats` | 服务端统计 |
| `POST` | `/api/sync/check` | 双向差异对比 |
| `POST` | `/api/sync/pull` | 批量拉取会话 |
| `POST` | `/api/sessions/upload` | 上传单条会话 |
| `POST` | `/api/sessions/upload-batch` | 批量上传 |
| `GET` | `/api/sessions` | 分页列表 |
| `GET` | `/api/sessions/search?q=` | 关键词搜索 |
| `GET` | `/api/sessions/:id/messages` | 获取消息内容 |

---

## 平台支持

| 平台 | 客户端 | 服务端 |
|------|--------|--------|
| macOS | ✅ 已验证 | ✅ |
| Windows | 🔧 理论支持 | ✅ |
| Linux | 🔧 理论支持 | ✅ |

## License

MIT
