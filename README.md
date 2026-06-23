# CodeI

终端优先的 AI 编程 Agent，用自然语言在本地仓库中读代码、改代码、跑命令、调试问题。

Rust 实现，单二进制分发，支持 OpenAI 兼容 API、Anthropic、vLLM 等 Provider，提供全屏 TUI、行式 REPL 与 SDK。

## 特性

- **多 Provider**：OpenAI、Anthropic、任意 OpenAI 兼容端点（vLLM、本地模型等）
- **内置工具**：`read` / `write` / `edit` / `grep` / `glob` / `list_dir` / `shell` / `definition`，以及子 Agent `task`
- **MCP**：通过 [Model Context Protocol](https://modelcontextprotocol.io/) 扩展外部工具
- **会话持久化**：SQLite 存储，支持恢复、导出、压缩
- **TUI**：基于 Ratatui 的全屏交互，流式输出、滚动、Slash 补全、剪贴板复制
- **安全策略**：破坏性工具可要求审批；Shell 支持超时与沙箱限制
- **国际化**：界面支持 `zh-CN` / `en-US`
- **可编程**：`codei-sdk` 供脚本与 CI 集成

## 快速开始

### 从源码构建

```bash
git clone https://github.com/007gzs/codei.git
cd codei
cargo build --release -p codei
./target/release/codei --help
```

### 初始化配置

```bash
codei config init
```

配置文件默认位于 `~/.config/codei/config.toml`。示例：

```toml
[defaults]
model = "gpt-4o"
provider = "openai"
temperature = 0.2
max_tokens = 8192
language = "zh-CN"

[providers.openai]
api_key_env = "OPENAI_API_KEY"
base_url = "https://api.openai.com/v1"

# 本地 vLLM / OpenAI 兼容服务
[providers.custom]
api_key = "empty"
base_url = "http://localhost:8000/v1"
api_style = "openai"
tool_format = "tools"   # vLLM 请使用 tools，不要用 functions
```

也可在配置中直接写 `api_key`（优先于 `api_key_env`）。

### 启动交互

```bash
# 全屏 TUI（默认）
codei

# 指定工作目录
codei --cwd /path/to/your/project

# 行式 REPL
codei --no-tui

# 单次提问并打印结果
codei -p "解释这个项目的结构"

# 自动批准工具调用
codei -y

# 调试日志（写入 ~/.local/share/codei/logs/debug.log）
codei --verbose
```

## 配置说明

| 配置项 | 说明 |
|--------|------|
| `defaults.provider` | 使用的 Provider 名称 |
| `defaults.model` | 默认模型 |
| `defaults.language` | UI 语言：`zh-CN` / `en-US` |
| `providers.*.base_url` | API 地址 |
| `providers.*.api_style` | `openai` 或 `anthropic` |
| `providers.*.tool_format` | `tools`（推荐）或 `functions`（旧版 function calling） |
| `agent.max_tool_rounds_per_turn` | 单轮最多工具调用次数 |
| `tools.shell.enabled` | 是否启用 shell 工具 |

项目级配置可放在 `.codei/config.toml`；项目说明可写在 `AGENTS.md` 或 `.codei/rules/`。

完整设计见 [docs/DESIGN.md](docs/DESIGN.md)。

## CLI 命令

```bash
codei                          # 交互模式
codei -p "你的问题"             # 单次执行
codei -c                       # 继续最近一次会话
codei -r <session-id>          # 恢复指定会话

codei config show              # 查看合并后的配置
codei config init              # 创建用户配置

codei session list             # 列出会话
codei session delete <id>      # 删除会话
codei session export <id>      # 导出 JSONL

codei mcp init                 # 创建 MCP 配置
codei mcp list                 # 列出 MCP 服务器
codei mcp add <name> -- <cmd>  # 添加 MCP 服务器
```

## TUI 快捷键

| 操作 | 按键 |
|------|------|
| 发送消息 | `Enter` |
| 滚动聊天 | `PageUp` / `PageDown`，`Home` / `End` |
| 选中复制（终端原生） | `Shift` + 鼠标拖动 |
| 复制全部聊天 | `Ctrl+Shift+Y` 或 `/copy` |
| 复制上一条助手回复 | `Ctrl+Shift+L` 或 `/copy last` |
| Slash 补全 | `Tab`，`↑` / `↓` |
| 退出 | `Ctrl+C` |
| 工具审批 | `y` 同意 / `n` 拒绝 |

## Slash 命令

在输入框中使用：

| 命令 | 说明 |
|------|------|
| `/help` | 显示帮助 |
| `/exit`, `/quit` | 退出 |
| `/clear` | 清空当前会话消息 |
| `/compact` | 压缩会话历史 |
| `/copy`, `/copy last` | 复制聊天内容 |
| `/model <name>` | 切换模型 |
| `/provider <name>` | 切换 Provider |
| `/session list` | 列出会话 |
| `/session new` | 新建会话 |
| `/session resume <id>` | 恢复会话 |

## 内置工具

| 工具 | 功能 |
|------|------|
| `read` | 读取文件（支持行范围） |
| `write` | 写入文件 |
| `edit` | 按片段替换编辑 |
| `grep` | 正则搜索代码库 |
| `glob` | 按模式匹配文件路径 |
| `list_dir` | 列出目录 |
| `shell` | 执行 Shell 命令 |
| `definition` | 查找符号定义 |
| `task` | 启动子 Agent 处理子任务 |

## MCP

在 `~/.config/codei/mcp.toml`（或 `codei mcp init` 生成）中配置 MCP 服务器，启动时自动连接并将其工具注册为 `mcp_<server>_<tool>`。

```bash
codei mcp add filesystem -- npx -y @modelcontextprotocol/server-filesystem /path/to/dir
```

## 开发

```bash
# 测试
cargo test --workspace

# Lint
cargo clippy --workspace -- -D warnings

# 仅构建 CLI
cargo build -p codei
```

### Crate 结构

```
crates/
├── codei-cli/       # 二进制入口
├── codei-tui/       # TUI
├── codei-agent/     # Agent 循环
├── codei-llm/       # LLM Provider 抽象
├── codei-tools/     # 工具注册与执行
├── codei-session/   # 会话与持久化
├── codei-commands/  # Slash 命令
├── codei-config/    # 配置加载
├── codei-mcp/       # MCP 客户端
├── codei-sdk/       # 程序化 API
└── codei-i18n/      # 国际化
```

## 许可证

Dual-licensed under [MIT](LICENSE) or [Apache-2.0](LICENSE-APACHE).
