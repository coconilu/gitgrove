# 一键发布

在 **Actions → Release → Run workflow** 选择默认主分支 master 和版本增量，点击运行一次即可。

| 选项 | 结果示例（当前为 1.2.3） |
| --- | --- |
| patch（默认） | 自动发布 1.2.4，适合修复与小改进 |
| minor | 自动发布 1.3.0，适合兼容的新功能 |
| major | 自动发布 2.0.0，适合不兼容的大版本 |
| none | 不升级版本，发布 / 重试当前版本；该触发提交必须已通过 CI |

正常流程无需手动批准版本 PR 的工作流、等待后点击合并，或再次触发 Release。

## 自动完成的步骤

```mermaid
flowchart LR
  A[选择增量并运行] --> B[同步版本和变更记录]
  B --> C[版本提交 CI]
  C --> D[自动合并版本 PR]
  D --> E[合并提交 CI]
  E --> F[Windows / macOS 构建和签名]
  F --> G[发布 Release 与更新清单]
```

版本 PR 仍作为版本修改记录保留，但由流程自动处理。只使用 GITHUB_TOKEN，不需要额外的 PAT、GitHub App 或仓库 auto-merge 开关，不调整现有主分支保护。

流程显式调用 CI 的 workflow_dispatch 并等待相同 SHA 的结果，避免依赖 GITHUB_TOKEN 创建 PR 后可能需要人工批准的 pull_request 运行。版本 PR 的 head SHA 与合并后的 SHA 分别验证，不能拿原版本或另一个提交的成功状态代替。最终构建、tag 与发布固定到同一个已通过 CI 的合并提交。

每次运行使用独立的 codex/release-<run-id> 分支，历史 release-v* 残留分支不会卡住新运行。同一次运行重试会验证并复用自己的版本分支 / PR，不覆盖其他分支；版本分支夹带源码或偏离自动生成的版本内容时停止。

## 失败与重试

| 情况 | 处理 |
| --- | --- |
| CI 失败、取消、跳过或超时 | 停止合并 / 发布，查看失败的 CI 运行 |
| 分支保护拒绝合并或主分支在运行期间发生变化 | 停止并给出错误；修复原因后在当前主分支重跑，不绕过保护 |
| 构建或上传失败 | 使用 Re-run failed jobs 重试；无需再次增加版本号 |
| 当前版本尚未发布但代码已合并 | 选择 none，发布通过 CI 的当前提交 |
| 版本已正式发布 | 跳过，不覆盖用户已收到的安装包 |
| Issue 以 completed 关闭 | 为事件的固定提交生成 vX.Y.Z-issueN 预发布，仍验证 CI |
| Issue 以 not_planned 关闭 | 不构建预发布 |

所有平台产物上传完成后才发布 Release 草稿。任何平台的安装包或 updater 签名缺失都会失败；预发布不占用 releases/latest。签名仍使用既有 TAURI_SIGNING_PRIVATE_KEY 和对应密码 Secret。

## 维护说明

仓库需允许 Actions 创建 PR：Settings → Actions → General → Allow GitHub Actions to create and approve pull requests。该仓库已启用。主分支仍要求 PR 与必需 check；若后续增加人工审批规则，需要同步调整发布权限设计，流程不会自行绕过。

默认 GITHUB_TOKEN 权限保持 read；自动版本任务需要 contents/write、pull-requests/write、actions/write（触发并读取 CI），发布任务需要 contents/write。CI 自身只有读取权限。自动任务只合并本次生成并验证的版本 PR，不会自动合并功能 PR。

校验命令：

- node --test .github/scripts/*.test.mjs
- pnpm -C app lint
- actionlint .github/workflows/ci.yml .github/workflows/release.yml

本地检查可验证编排和拒绝路径；在线的签名构建与发布需要在工作流合并后通过下一次实际发版验证。

GitHub 官方说明：[GITHUB_TOKEN 与 workflow_dispatch 的触发行为](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow)。
