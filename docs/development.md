# 开发指南

## 环境要求

- **Node.js 24**（CI 基线）
- **pnpm 10.24.0**（`packageManager` 字段锁定，建议 `corepack enable`）
- **Rust stable**（无 `rust-toolchain.toml`，用工具链默认 stable；edition 2021）
- Tauri 2 系统依赖：Windows 需 WebView2（Win10/11 自带）；macOS 需 Xcode Command Line Tools
- 本机已安装 **git**（应用直接调用用户 git 二进制）

## 常用命令

```powershell
pnpm -C app install          # 安装依赖
pnpm -C app tauri dev        # 开发模式（自动起 vite + Tauri 窗口，devUrl localhost:1420）
pnpm -C app tauri build      # 打包当前平台安装器（Windows NSIS / macOS dmg+app）

pnpm -C app dev              # 仅前端 vite dev server
pnpm -C app build            # 前端构建（tsc && vite build）
pnpm -C app test             # 前端单测（node --test tests/*.test.mjs）
pnpm -C app lint             # Biome 检查整个仓库
pnpm -C app lint:fix         # Biome 自动修复
```

Rust 侧：

```powershell
cargo check --manifest-path app/src-tauri/Cargo.toml
cargo test  --manifest-path app/src-tauri/Cargo.toml --lib
```

## 仓库结构

```
app/src/            React 渲染层（组件、zustand store、invoke 封装）
app/src-tauri/      Rust 主进程（git/GitHub API/fs/Agent 集成）
app/tests/          前端单测（node --test）
.github/scripts/    发版脚本（bump-version、gen-latest-json、prepare-release，均带测试）
.github/workflows/  ci.yml / release.yml
docs/               技术文档与 GitHub Pages 站点
方案-GitHub项目管理应用.md   产品/技术方案（定位、竞品、需求）
```

## 测试

- 前端：`app/tests/*.test.mjs` 直接 import TS 源，覆盖 `navigation.ts` 纯函数和 `updates.ts` 更新状态机；
- 发版脚本：`.github/scripts/*.test.mjs`；
- Rust：`cargo test --lib`（含 workflow metadata parser、agents 模块）；
- Agent 集成的真实 GUI 验收（默认不跑，需本机装好 Codex/Kimi Code）：

```powershell
$env:GITGROVE_AGENT_TARGET = 'C:\existing-project'
$env:GITGROVE_AGENT_TOOL = 'codex'   # 或 kimi
cargo test --manifest-path app/src-tauri/Cargo.toml --lib agents::tests::desktop_open -- --ignored --exact --nocapture
```

更多边界说明见 [open-in-agents.md](open-in-agents.md#可重复验收)。

## CI / CD

**ci.yml**（PR 与 master push，windows-latest）：pnpm 安装 → Biome lint → 前端 build → 发版脚本测试 → 前端单测 → `cargo check` → `cargo test --lib`。

**release.yml**（一键发版）：手动 dispatch（patch/minor/major）或 master CI 成功后触发；流程为 prepare（bump 版本 + 自动合并版本 PR）→ version（校验）→ build（Windows + macOS 矩阵打包）→ publish（生成 updater manifest `latest.json`、创建并发布 GitHub Release）。详细操作见 [.github/RELEASING.md](../.github/RELEASING.md)。

应用内置 `tauri-plugin-updater`，从 `releases/latest/download/latest.json` 检查更新，用户可在 About 面板查看版本并安装更新。

## 提交约定

- commit/PR 标题用 conventional commits（`feat:` / `fix:` / `chore:` / `ci:` / `test:`）；
- master 受分支保护，改动一律走 PR，CI 绿了才合并；
- Windows 下新增子进程调用必须 `CREATE_NO_WINDOW`，禁止弹窗打断用户。
