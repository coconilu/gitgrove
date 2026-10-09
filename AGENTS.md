# AGENTS.md

本文件面向在本仓库工作的 AI agent 与新加入的贡献者，记录项目结构、常用命令与协作流程。详细背景见 [README](README.md)、[开发指南](docs/development.md) 与[发布流程](.github/RELEASING.md)。

## 项目概览

- **GitGrove**：以「项目 ↔ worktree ↔ issue/PR」关系为主轴的 GitHub 桌面客户端，技术栈 **Tauri 2**（Rust 主进程）+ **React 19** + Tailwind 4 + zustand。
- 代码全部在 `app/` 下：前端 `app/src/`（React 渲染层），后端 `app/src-tauri/`（Rust 主进程，git/GitHub API/fs/Agent 集成）。
- 包管理器是 **pnpm**（版本由 `app/package.json` 的 `packageManager` 字段锁定），lint/format 用 **Biome**，前端单测是 **node --test**（跑 `app/tests/*.test.mjs`）。

## 常用命令

以下命令都在仓库根目录执行：

```powershell
pnpm -C app install          # 安装依赖
pnpm -C app test             # 前端单测（node --test tests/*.test.mjs）
pnpm -C app build            # 前端构建（tsc && vite build）
pnpm -C app lint             # Biome 检查整个仓库（CI 同款命令）
pnpm -C app lint:fix         # Biome 自动修复
pnpm -C app tauri dev        # 开发模式（需要 Rust toolchain）
pnpm -C app tauri build      # 打包当前平台安装器
```

Rust 侧：

```powershell
cargo check --manifest-path app/src-tauri/Cargo.toml
cargo test  --manifest-path app/src-tauri/Cargo.toml --lib
```

CI（`.github/workflows/ci.yml`）在 PR 和 master push 上运行：Biome lint → 前端 build → 发版脚本测试 → 前端单测 → `cargo check` → `cargo test --lib`。提交前至少跑通 `pnpm -C app lint`、`pnpm -C app test` 和 `pnpm -C app build`。

## Issue / PR 流程

1. 有明确改动点先提 issue，描述清楚问题与期望行为；
2. 从 master 拉分支开发，分支名带上 issue 编号或主题（如 `feat/sidebar-private-tag-issue106`）；
3. 开 PR，正文用 `Closes #N` 关联 issue，合并后 issue 自动关闭；
4. master 受分支保护，改动一律走 PR，CI 绿了才合并；commit/PR 标题用 conventional commits（`feat:` / `fix:` / `chore:` / `ci:` / `test:`）。

## 发版流程（重要：合并 ≠ 发版）

**master 合并不会自动发版。** `release.yml` 只支持手动 `workflow_dispatch` 触发，没有任何自动发版路径。

需要发版时手动执行（或在 Actions 页面点 Run workflow）：

```powershell
gh workflow run release.yml -f bump=patch    # 或 minor / major / none
```

- `patch` / `minor` / `major`：升级版本号并发布对应 Release；
- `none`：不升版本，发布/重试当前版本（该提交必须已通过 CI）。

**Agent 行为约定：一个 issue 或 PR 处理完成、合并之后，必须主动询问用户「要不要发版」，不要默认跳过。** 这是本仓库的固定收尾动作——历史上曾多次出现合并后无人发版、用户以为新功能已上线的情况。

## 代码风格

- 缩进用 **tab**（Biome `indentStyle: "tab"`），JS 字符串用双引号；
- 注释和标识符随现有代码用**中文**；注释克制，只在解释「为什么」或非显而易见的边界时写，不复述代码做了什么；
- commit message 和 PR 描述用中文正文 + conventional commits 前缀。

## Windows 环境注意事项

- 在这台 Windows 机器上执行会拉起子进程的命令（构建、测试、渲染、headless 浏览器等）一律后台运行，**绝不能弹出新终端窗口打断用户**；
- Rust 侧新增子进程调用必须带 `CREATE_NO_WINDOW`；
- 调试浏览器一律 `--headless`，不启动带界面的程序。
