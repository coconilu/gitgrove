# Changelog

## v1.3.0（2026-09-12）

- Merge pull request #81 from coconilu/chore/tower-merge-m27
- Merge branch 'feat/done-20-80'
- refactor(pm): done 列统一按 closedAt 倒序，展开=连续追加；types.ts 补 closedAt (#80)
- feat(pm): done 列折叠——最近 20 张 + 分批展开，补 closedAt 排序字段 (#80)

## v1.2.9（2026-09-11）

- Merge pull request #78 from coconilu/chore/tower-merge-m26
- Merge branch 'feat/swr-77'
- fix(pm): 回应 review r3——items 变更与 saveStatuses 的作废 keys 补全 (#77)
- fix(pm): 回应 review r2——变更作废覆盖各自 feed 键、清理过期术语与死代码 (#77)
- fix(pm): 回应 review r1——缓存命中首帧即显、有旧数据时刷新失败走 inline 横幅、竞态判定改请求发出时刻 (#77)
- feat(pm): 看板接入 SWR——缓存先显、旋转加载提示、拖拽竞态防护 (#77)

## v1.2.8（2026-09-11）

- Merge pull request #75 from coconilu/chore/tower-merge-m25
- Merge branch 'feat/gh-cli-74'
- fix(pm): parse_close_refs 改按 char_indices 遍历，中文 PR 文本不再 panic (#74)
- fix(github): try_gh_cli 加 5s 进程超时杀树 + 同步链路逐阶段计时诊断 (#74)

## v1.2.7（2026-09-11）

- Merge pull request #72 from coconilu/chore/tower-merge-m24
- Merge branch 'feat/dsh-url-71'
- fix(agents): dsh 打开改用捕获的认证 URL 并修正复用候选顺序 (#71)

## v1.2.6（2026-09-10）

- Merge pull request #69 from coconilu/chore/tower-merge-m20-m23
- Merge branch 'feat/ci-flake-real-powershell-53'
- Merge branch 'feat/dsh-open-68'
- fix(agents): StartedServer 仅对确认存活的子进程按 PID 清理进程树（r2 review P2）(#68)
- fix(agents): DSH 实例复用与进程树清理（r1 review P2×3）(#68)
- feat(agents): 「在…中打开」接入 DSH（deepseek-harness）(#68)
- Merge pull request #67 from coconilu/chore/tower-merge-m19
- style: biome organize imports in PmPanel
- Merge branch 'feat/66'
- fix(pm): keyring 恢复 windows-native，修复看板同步 0 卡片 (#66)
- fix(launch): real_powershell 测试超时 15s→60s + 超时重跑一次 (#53)
- Merge pull request #65 from coconilu/chore/tower-merge-m18
- Merge branch 'feat/ci-rust-cache-prepare-64'
- fix(ci): prepare job 补 checks:read 权限，rust-cache 注释如实写明命名空间互不相通
- perf(ci): rust-cache 加速 Rust job，版本 PR 快速通道合并，去掉 workflow_run 空跑 (#64)

## v1.2.5（2026-09-10）

- Merge pull request #62 from coconilu/chore/tower-merge-m16-m17
- Merge branch 'feat/sync-github-61'
- Merge branch 'feat/swr-issues-pr-actions-60'
- feat(ui): Issues/PR/Actions 页签缓存先显，SWR 后台刷新 (#60)
- fix(pm): 看板本地数据先渲染，GitHub 同步后台化 + HTTP 超时兜底 (#61)

## v1.2.4（2026-09-10）

- Merge pull request #58 from coconilu/chore/tower-merge-m14-m15
- Merge branch 'feat/p2-github-57'
- feat(pm): GitHub 同步卡片展示 + 手动锁定 (#57)
- Merge branch 'feat/p2-github-issue-57'
- fix(pm): migration 原子化 + ALTER 幂等守卫，消除半迁移永久 wedge
- feat(pm): GitHub issue → 看板同步引擎 + manual_lock 字段 (#57)

## v1.2.3（2026-09-10）

- Merge pull request #55 from coconilu/chore/tower-merge-m13
- Merge branch 'feat/54'
- fix(pm): PmStore::open 设置 busy_timeout=3s，二次启动撞写锁不再 panic
- feat(single-instance): 二次启动只激活已有实例 (#54)

## v1.2.2（2026-09-10）

- Merge pull request #49 from coconilu/chore/tower-merge-m12
- Merge remote-tracking branch 'origin/master' into chore/tower-merge-m12
- Merge branch 'feat/windows-48'
- feat(windows): 系统托盘（关闭进托盘）+ 窗口状态记忆 (#48)

## v1.2.1（2026-09-10）

- 无提交记录

## v1.2.0（2026-09-09）

- Merge pull request #46 from coconilu/chore/tower-merge-m11
- Merge branch 'feat/projects-v2-stub-list-get-projects-v2'
- Merge pull request #45 from coconilu/chore/tower-merge-m8-m10
- chore(pm): 删除 projects_v2 临时 stub（M8 前端已摘除调用）
- Merge branch 'feat/pm-commands-pm-list-statuses-pm-update-s'
- Merge branch 'feat/pm-p0'
- fix(pm): 提交 ProjectsPanel.tsx 删除；create/move 后刷新里程碑统计；清理 list-tools 死规则
- feat(pm): 新增 pm_list_statuses / pm_update_statuses 看板列管理 command
- feat(pm): 自研项目管理 P0 前端 — 看板 + 里程碑视图，退役 Projects v2 面板
- Merge pull request #44 from coconilu/chore/tower-merge-m9
- Merge branch 'feat/pull-42'
- Merge pull request #43 from coconilu/chore/tower-merge-m7
- fix: 分支同步状态刷新前先 fetch + 新增 ff-only 拉取入口 (#42)
- Merge branch 'feat/pm-sqlite-store-tauri-commands-p0'
- fix(pm): 加回 projects_v2 命令临时 stub，避免 M8 前端摘除前运行时回归
- feat(pm): 自研项目管理 P0 后端 — SQLite store + 模型 + Tauri commands
- Merge pull request #41 from coconilu/docs/pm-self-hosted-plan
- docs: 自研项目管理方案 + 看板/里程碑视图草稿 (#29 #40)

## v1.1.0（2026-09-09）

- Merge pull request #36 from coconilu/chore/tower-merge-35
- Merge branch 'feat/zcode-35'
- fix: ZCode 用 --open-workspace 传项目目录；explorer 统一反斜杠路径 (#35)
- Merge pull request #34 from coconilu/chore/tower-merge-29-30-31
- Merge branch 'feat/radix-ui-components'
- fix: review r3 — 恢复 .list-tools > select 规则，ProjectsPanel 原生 select 不丢宽度约束
- feat: ProjectDetail 任务状态筛选下拉换为 ui/Select（#31 收尾）
- fix: review r1 — biome 格式化 DialogBody 签名；表单内 ui-select-trigger 对齐 .input 字号/内边距
- feat: Radix UI Primitives 基础组件（Select/Dialog/DropdownMenu/Button/Input）
- Merge branch 'feat/branch-panel-toolbar'
- feat: 分支面板工具栏——过滤远程分支开关与删除已合并分支（#30 UI）
- feat: 注册 merged 分支 commands + api.ts 前端封装（#30）
- feat: 已合并分支查询/批量删除与删除预览（#30 后端）
- Merge branch 'feat/projects-panel-ui'
- Merge branch 'master' of https://github.com/coconilu/gitgrove
- fix: review r1 三处 P2 + nit（closed 可选、引导重试先刷新登录态、看板仅按 Status、force 一次性、truncated 提示）
- feat: Projects 面板接入 M1 真实接口（listProjectsV2/getProjectV2 + scope 引导）
- feat: Projects 页签只读面板 UI（本地契约，待与 M1 联调）
- Merge branch 'feat/projects-v2-backend'
- fix: review round 1 的 5 个 P2
- feat: Projects V2 GraphQL 只读访问层 + project scope 检测

## v1.0.6（2026-09-09）

- Merge pull request #32 from coconilu/feat/open-in-menu-rework
- Merge remote-tracking branch 'origin/master' into feat/open-in-menu-rework
- fix: review 反馈——githubUrl 校验 provider 为 github；resolve_in 父目录回退覆盖 zcode
- feat: 打开方式统一收拢到「在…中打开」菜单

## v1.0.5（2026-09-08）

- Merge pull request #26 from coconilu/fix/markdown-images-html-rewrite
- Merge branch 'master' into fix/markdown-images-html-rewrite
- fix: Markdown 图片改写移到 HTML 字符串层面，修复重渲染后再次裂图

## v1.0.4（2026-09-08）

- Merge pull request #24 from coconilu/fix/markdown-local-images
- fix: 本地 Markdown 预览支持相对路径图片
- Merge pull request #23 from coconilu/docs/demo-gif
- docs: add real-app demo GIF to README and landing page
- Merge pull request #21 from coconilu/chore/license
- chore: add MIT license
- Merge pull request #20 from coconilu/docs/promotion
- docs: add README, architecture/development docs and GitHub Pages landing

## v1.0.3（2026-09-07）

- Merge pull request #17 from coconilu/codex/issue-16-ui-polish
- fix(ui): dismiss tool hints before layout-changing actions
- feat(ui): polish tool buttons and active worktree state (#16)

## v1.0.2（2026-09-07）

- fix: improve sidebar navigation and panel resizing (#14)

## v1.0.1（2026-09-07）

- [修复] 自动批准并等待版本 PR 的实际 CI (#12)
- ci: automate one-click version release through protected CI (#10)

## v1.0.0（2026-09-07）

- feat: add dismissible updates and About settings (#8)
- Merge pull request #6 from coconilu/codex/issue-4-open-in-agents
- test: make parallel agent fixtures uniquely named
- Merge remote-tracking branch 'origin/master' into codex/issue-4-open-in-agents
- fix: verify Kimi TCP peer identity before sending credentials
- fix: align window theme and isolate workspace controls
- fix: restore open menu focus after launch failure
- feat: open projects and worktrees in Codex and Kimi Code

## v0.1.2（2026-09-06）

- Merge pull request #3 from coconilu/codex/editor-terminal-settings
- fix: isolate terminal console handles and detect Linux terminal processes
- test: compare canonical PowerShell working directories
- feat: configure default editors and terminals
- Merge pull request #2 from coconilu/codex/ui-workspace-release-pr
- feat: 重构项目工作区并兼容主分支保护发布

## v0.1.1（2026-09-06）

- release: bump 优先直推 master，被推保护拒绝才回退 PR 流程
- 产品定名 GitGrove，启用 git 树新图标

