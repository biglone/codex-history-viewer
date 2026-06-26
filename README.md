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
