# Codex History Viewer

一个用于查看和搜索 [Codex Desktop](https://openai.com/codex) 历史会话的本地桌面工具，基于 Tauri（Rust + HTML/CSS/JS）构建。

## 功能特性

- 📋 **会话列表** — 分页浏览全部历史会话，展示标题、时间、所属项目、模型
- 📁 **按项目浏览** — 按工作目录（cwd）分组，快速定位某个项目的所有对话
- 🔍 **三层搜索**
  - **列表过滤**：当前页即时过滤，显示匹配数（如 `5/30`），纯客户端无延迟
  - **会话内搜索**：展开详情后可搜索对话内容，黄色高亮匹配，`↑↓` 导航跳转，显示 `2/7` 进度
  - **全局搜索**：跨所有会话文件（SQLite + JSONL）深度检索
- 💬 **对话详情** — 侧边栏展示完整多轮问答，支持代码块/粗体 Markdown 渲染
- ⌨️ **键盘快捷键** — `Cmd+F`（上下文感知）/ `Cmd+G`（全局搜索）/ `Esc` 关闭

## 数据来源

直接读取 Codex Desktop 本地数据，无需任何服务器或网络：

```
~/.codex/
├── state_5.sqlite      # 会话索引（标题、时间、项目、模型）
└── sessions/           # 完整对话内容（JSONL 格式）
```

## 技术栈

- **前端**：HTML + CSS + JavaScript（无框架）
- **后端**：Rust（rusqlite 读取 SQLite，walkdir 遍历 JSONL）
- **框架**：[Tauri v2](https://tauri.app/)

## 平台支持

| 平台 | 状态 |
|------|------|
| macOS | ✅ 已验证 |
| Windows | 🔧 理论支持（待测试） |
| Linux | 🔧 理论支持（待测试） |

## 开发环境要求

- [Rust](https://rustup.rs/) 1.77+
- [Node.js](https://nodejs.org/) 18+
- Tauri 系统依赖（见 [官方文档](https://tauri.app/start/prerequisites/)）

## 快速开始

```bash
# 安装依赖
npm install

# 开发模式启动
npm run tauri dev

# 打包发布版本
npm run tauri build
```

## 键盘快捷键

| 快捷键 | 说明 |
|--------|------|
| `Cmd+F` | 列表页：聚焦过滤栏；详情面板开着：聚焦会话内搜索 |
| `Cmd+G` | 打开全局搜索 |
| `Enter` | 跳到下一处搜索匹配 |
| `Shift+Enter` | 跳到上一处搜索匹配 |
| `Esc` | 关闭当前搜索 / 关闭详情面板 |

## License

MIT
