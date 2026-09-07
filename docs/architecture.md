# 架构文档

> 对应版本：v1.0.x。产品定位与需求见 [方案-GitHub项目管理应用.md](../方案-GitHub项目管理应用.md)。

## 总览

```
┌────────────────────────── GitGrove 窗口 ──────────────────────────┐
│  React 渲染层（app/src）                                          │
│    Sidebar / ProjectDetail / MyGitHub / FileTreePanel / …         │
│    zustand 单一 store · invoke() 薄封装（api.ts，约 30 个命令）    │
├───────────────────────────────────────────────────────────────────┤
│  Rust 主进程（app/src-tauri/src）                                 │
│    github.rs · projects.rs · git.rs · fsx.rs · store.rs           │
│    launch.rs · agents.rs · window_theme.rs                        │
└───────────────────────────────────────────────────────────────────┘
        │                │                  │
   用户 git 二进制   GitHub API        本地文件系统 / keyring
```

核心原则：**git、网络、文件操作全部在 Rust 主进程**，渲染层只发命令（invoke）收事件；token 不出 Rust 边界。

## 渲染层（app/src）

单一 zustand store（`store.ts`，不拆 slice），主要状态域：

| 状态域 | 内容 |
| --- | --- |
| 认证 | `auth` / `authLoaded` |
| 导航 | `view`（projects/github）、`sel`、`tab`、`visits`（每项目记忆选中态）、`groupBy`、`expanded` |
| 布局 | `layout`（侧栏/文件树开关与宽度，持久化 localStorage `gitgrove.layout.v1`） |
| 数据 | `projects`、`myRepos`、`ci`（per-checkout CI 状态，4 并发分批拉取）、`statusMaps`（git status 角标） |
| 瞬态 | `cloneProgress`、`toastMsg`、`dialog` |

导航与选中逻辑抽成纯函数集中在 `navigation.ts`（navigate、selectionProject、ciStatus、worktreePath 等），被 `app/tests` 单测覆盖。所有 refresh action 带请求序号防竞态。

组件与职责见源码 `app/src/components/`，关键点：

- `Sidebar.tsx`：项目 → checkout 两级列表，主 clone（`primary`）与 worktree 同一组件渲染，仅徽章不同；
- `FileTreePanel.tsx`：右侧文件树，目录懒加载（展开才 `read_dir`），文件行带 M/A/D 角标；
- `FilePreview.tsx` + marked：Markdown 预览；
- `UpdateControls.tsx`：tauri-plugin-updater 封装（更新 Toast + About 面板），状态机逻辑在 `updates.ts`，有单测。

`api.ts` 是对 `@tauri-apps/api/core` 的 `invoke` 薄封装，约 30 个命令分六组：认证、My GitHub、clone/项目、worktree/分支、文件系统、Issues/PRs/Actions。前后端类型经 serde camelCase 对齐（`types.ts`）。

## 主进程（app/src-tauri/src）

| 模块 | 职责 |
| --- | --- |
| `lib.rs` | Tauri Builder、`AppState`（内存 token + reqwest client）、注册全部 invoke 命令 |
| `github.rs` | GitHub API：token 管理；仓库列表用 GraphQL 聚合（省 rate limit），issues/PRs/Actions 走 REST |
| `projects.rs` | clone/add/list/remove 项目，worktree 增删锁，从 issue/PR 开 worktree，create_pr；clone 进度经事件推给前端 |
| `git.rs` | 一律调用户 git 二进制（不绑 libgit2）；Windows 全部 `CREATE_NO_WINDOW`；`GIT_TERMINAL_PROMPT=0` 防挂起；token 以 `x-access-token` 注入 https URL |
| `fsx.rs` | read_dir、read_file_preview、checkout_status、trash_path（回收站删除）、reveal_in_explorer |
| `store.rs` | `projects.json` 持久化、旧字段迁移、项目配色哈希 |
| `launch.rs` | 探测已安装编辑器/终端、默认打开方式偏好、`open_in_editor/terminal/explorer` |
| `agents.rs` + `agents/transport.rs` | Open in Agent（Codex / Kimi Code）。transport 是 Windows 专用 HTTP/1 传输：先经连接表核对对端进程 PID/映像/用户身份，确认后才在同一 socket 上发送凭证，详见 [open-in-agents.md](open-in-agents.md) |
| `window_theme.rs` | Windows 深色标题栏（DWM） |

关键依赖：`tauri 2` + `tauri-plugin-opener/updater`、`reqwest 0.12`（rustls + system-proxy）、`tokio`、`keyring 3`、`trash 5`、`serde_yaml_ng`；Windows 专用 `hyper 1`、`windows-sys 0.61`。**没有** octocrab、libgit2、notify（无文件监听，状态靠前端主动刷新 + 轮询）。

## 数据模型与持久化

- **Project**：`providerIdentity {provider, owner, repo}` 可空 —— 任意本地 git 仓库（无 origin、GitLab/Gitee 等）都可注册为项目，GitHub 相关 tab 降级为空状态，本地 git 功能不受影响。
- **Checkout**：主 clone 与 worktree 统一建模，`id = ${projectId}::${path}`，`isPrimary` 区分；灵魂字段 `linkedWorkItem {type: issue|pr, number, title, url}` 把 worktree 和 GitHub 工作项持久化关联。
- **持久化位置**：项目/checkout 元数据存 `%APPDATA%/gh-projects/projects.json`（非 Windows 为 `~/.config/gh-projects/`），worktree 实体以 `git worktree list` 为准；编辑器/终端偏好同目录；前端仅布局进 localStorage。**GitHub 侧数据不进本地库**，走内存缓存 + 刷新策略。
- **目录约定**：clone 默认 `~/gh-projects/<repo>/`，worktree 落在兄弟目录 `<repo>.worktrees/<branch-slug>/`，避免污染项目目录。

## 认证与安全

- 认证方式为 **PAT**（Personal Access Token，需 `repo` + `workflow` scope），前端输入后由 Rust 校验并存入**系统 keyring**（service=`gh-projects`）。
- 取 token 顺序：内存 → keyring → **回退复用 `gh` CLI 已登录 token**（成功后写回 keyring）。
- token 全程只在 Rust 侧：HTTP 授权、git https 注入均在主进程完成，不通过 Tauri 命令返回给前端，不进日志。
- Agent 集成的凭证边界（TCP 对端身份核验、server.token 不出 Rust 等）见 [open-in-agents.md](open-in-agents.md)。

## 跨平台注意点

- Windows：所有子进程 `CREATE_NO_WINDOW` 不弹窗；DWM 深色标题栏；路径处理一律平台无关 API。
- 删除一律走系统回收站（`trash` crate），文案按平台区分。
- 双平台 CI/打包：GitHub Actions matrix（windows-latest + macos-latest），见 [开发指南](development.md)。
