# 一键发布

在 **Actions → Release → Run workflow** 选择默认主分支 master 和版本增量，点击运行一次即可。

| 选项 | 结果示例（当前为 1.2.3） |
| --- | --- |
| patch（默认） | 自动发布 1.2.4，适合修复与小改进 |
| minor | 自动发布 1.3.0，适合兼容的新功能 |
| major | 自动发布 2.0.0，适合不兼容的大版本 |
| none | 不升级版本，发布 / 重试当前版本；该触发提交必须已通过 CI |

正常流程无需手动批准版本 PR 的工作流、等待后点击合并，或再次触发 Release。

触发方式只有手动 `workflow_dispatch`（即上面的入口）。历史触发器都已删除：`workflow_run`（master CI 成功后自动发版）与 `issues: [closed]`（关闭 issue 出预发布）都不存在了——手动 dispatch 覆盖全部发版路径，含 none 重试，保留自动触发器只会产出空跑（`version: success` / `build: skipped`）或与手动发版重复的运行。

## 自动完成的步骤

```mermaid
flowchart LR
  A[选择增量并运行] --> B[同步版本和变更记录]
  B --> C[版本提交 CI（master 已绿时不等）]
  C --> D[自动合并版本 PR]
  D --> E[合并提交 CI]
  E --> F[Windows / macOS 构建和签名]
  F --> G[发布 Release 与更新清单]
```

版本 PR 仍作为版本修改记录保留，但由流程自动处理。只使用 GITHUB_TOKEN，不需要额外的 PAT、GitHub App 或仓库 auto-merge 开关，不调整现有主分支保护。

流程等待版本 PR 对应的 pull_request CI；如 GitHub 将其置于 action_required，由发布任务在核对仓库、PR、提交与版本文件后调用官方审批接口，再等待该 PR 的真实检查通过。单独 workflow_dispatch 成功无法代替被审批阻塞的 PR 检查。合并后才显式 workflow_dispatch CI 验证新的主分支提交；构建、tag 与发布固定到这个已通过 CI 的 SHA。

master HEAD 已有绿 CI 时走快速通道：版本 PR 只改版本文件、源码与主分支完全一致，于是跳过等待版本 PR 的完整 CI（原路径里这一项占了发版时长的大头），改为本地校验两个版本文件可解析且版本一致、`biome check` 通过，再按分支保护合并。分支保护要求版本 PR 自身的必需 check 时，直接合并会被拒（`Required status check ... is expected.`，HTTP 405）：命中该错误就回退为等待该 PR 的实际 CI，成功后再合并，不无脑重试合并、也不绕过保护。主分支 HEAD 没有绿 CI 时走原路径（等待版本 PR 的完整 CI）。读取 CI 状态需要 prepare 任务的 `checks: read` 权限。

每次运行使用独立的 codex/release-<run-id> 分支，历史 release-v* 残留分支不会卡住新运行。同一次运行重试会验证并复用自己的版本分支 / PR，不覆盖其他分支；版本分支夹带源码或偏离自动生成的版本内容时停止。

## 失败与重试

| 情况 | 处理 |
| --- | --- |
| CI 失败、取消、跳过或超时 | 停止合并 / 发布，查看失败的 CI 运行 |
| 分支保护拒绝合并或主分支在运行期间发生变化 | 停止并给出错误；修复原因后在当前主分支重跑，不绕过保护 |
| 构建或上传失败 | 使用 Re-run failed jobs 重试；无需再次增加版本号 |
| 当前版本尚未发布但代码已合并 | 选择 none，发布通过 CI 的当前提交 |
| 版本已正式发布 | 跳过，不覆盖用户已收到的安装包 |

所有平台产物上传完成后才发布 Release 草稿。任何平台的安装包或 updater 签名缺失都会失败。预发布入口随触发器一起删除：`release-target.mjs` 不再产出预发布标记，`version` job 也不再输出该字段，publish 总是把 Release 发布为正式 latest。签名仍使用既有 TAURI_SIGNING_PRIVATE_KEY 和对应密码 Secret。

## 构建提速与时长基准

- Rust 缓存：`ci.yml` 与 `release.yml` 的所有 Rust job（`cargo check`、Tauri build）都挂 Swatinem/rust-cache@v2。`workspaces` 指向 `app/src-tauri`（monorepo 里不是仓库根），`prefix-key` 按 workflow 区分：`gitgrove-CI-*` 与 `gitgrove-Release-*` 是互不相通的命名空间——CI 的 `cargo check` 只产出 rmeta，暖不了 release 的完整编译，因此首次发版仍是冷缓存。CI 侧另有 `save-if`，只有 master 的 run 写缓存，PR 分支不会把缓存池灌满。
- prepare 快速通道：见上文「自动完成的步骤」，含分支保护 405 的回退路径。
- 触发器精简：`workflow_run` 与 `issues: [closed]` 已从 `on:` 移除，只剩手动 dispatch（原因见开头）。

时长基准（同仓库双平台发版实测，issue #64 落地前后）：

| 场景 | 发版总时长 |
| --- | --- |
| 提速前（等版本 PR 完整 CI + 无 Rust 缓存） | 约 22min |
| 冷缓存首版（Rust 依赖全量编译） | 13-15min |
| 热缓存 + 快速通道 | 8-14min（v1.2.7 实测约 8min） |

冷缓存首版偏慢属预期，不算回归。剩余大头是 Windows 的 Tauri build，macOS 与它并行，publish（生成 latest.json 并发布 Release）约 15s。

## 维护说明

仓库需允许 Actions 创建 PR：Settings → Actions → General → Allow GitHub Actions to create and approve pull requests。该仓库已启用。主分支仍要求 PR 与必需 check；若后续增加人工审批规则，需要同步调整发布权限设计，流程不会自行绕过。

默认 GITHUB_TOKEN 权限保持 read；自动版本任务需要 contents/write、pull-requests/write、actions/write（审批本次版本 PR 的 CI、触发和读取检查），发布任务需要 contents/write。CI 自身只有读取权限。自动任务只批准和合并本次生成并验证的版本 PR，不会自动批准外部 PR 或合并功能 PR。

校验命令：

- node --test .github/scripts/*.test.mjs
- pnpm -C app lint
- actionlint .github/workflows/ci.yml .github/workflows/release.yml

本地检查可验证编排和拒绝路径；在线的签名构建与发布需要在工作流合并后通过下一次实际发版验证。

GitHub 官方说明：[GITHUB_TOKEN 与 workflow_dispatch 的触发行为](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)。
