# Changelog

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

