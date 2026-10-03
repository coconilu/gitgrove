# 在 Codex / Kimi Code 中打开

## 使用方式

克隆成功后会进入项目页，点击顶部“在…中打开”。已有项目、当前选中的工作树，以及 GitHub 列表中已克隆的项目也提供此入口。菜单保留配置中选择的默认编辑器和终端。

Windows 启用全部 Agent 图形界面入口；macOS 启用 Codex 与 Kimi Code Desktop，Kimi Code Web / DSH 的适配在 [issue #102](https://github.com/coconilu/gitgrove/issues/102) 跟进，暂不启用。Linux 仍可使用原有编辑器和终端。前后端按工具分别判断支持状态，不能通过直接调用后端绕过平台限制。

| 工具 | 打开行为 | 失败后处理 |
| --- | --- | --- |
| Codex | Windows 发送 `codex://new?path=<编码后的绝对目录>`；macOS 使用 `/usr/bin/open -b com.openai.codex <绝对目录>`，通过原生目录打开事件交接项目 | Windows 安装或修复 `codex` 协议；Mac 安装并正常启动桌面应用。Mac 按 Bundle ID 定位，不依赖 CLI、终端 PATH 或应用显示名称。应用没有完成回执，只提示“已发送打开请求”，需在 Codex 中确认 |
| Kimi Code | 复用兼容的本地 Web 实例；没有实例时隐藏启动 `kimi web`，注册或复用工作区，复用最近的非归档空闲会话，无会话时创建空会话，然后打开浏览器 | 按菜单错误提示检查安装、默认浏览器、鉴权、接口兼容性或 Web 服务状态；可手动运行 `kimi web` 后重试 |
| Kimi Code Desktop | Windows 定位 `Kimi Code.exe`（协议注册表优先，兜底系统/用户安装目录），以 `--workspace=<绝对目录>` 启动。Mac 检测 `/Applications/Kimi Code.app` 或 `~/Applications/Kimi Code.app` 的可执行文件，再用 `/usr/bin/open -n -a <bundle> --args --workspace=<绝对目录>` 启动；已运行实例由单实例锁转发工作区 | 未检测到安装时菜单项禁用；安装完成后重新展开菜单即可检测。Mac 等待启动器返回结果，失败或超时显示错误。应用没有完成回执，只提示“已发送打开请求”，需在 Kimi Code Desktop 中确认 |

Kimi 发现目录为 `KIMI_CODE_HOME/server/instances`，未配置时使用用户主目录下的 `.kimi-code`。GitGrove 和 Kimi 必须使用同一个绝对 `KIMI_CODE_HOME`。启动器从 PATH 和默认 `.kimi-code/bin/kimi.exe` 查找程序；更新环境变量后需要重启 GitGrove。

## 接口和凭证边界

- Codex 深链接是本机安装包实现支持的兼容入口，未获得公开稳定性承诺。
- Kimi Code Desktop 的 `--workspace` 启动参数同样未获得公开稳定性承诺（与 codex:// 同定性，实测自 1.0.2 的 Windows Jump List「最近工作区」行为）；版本升级后若打不开目标目录，按失败后处理排查。Kimi Code 的 Web 入口保留不变，两者共存。
- Mac 的桌面入口用绝对系统命令和独立 argv 传递目录，不经过 shell。Kimi 必须带 `-n`：普通 `open --args` 在应用已运行时不会转发启动参数。它借助第二个进程转发给现有窗口，不要求退出正在工作的应用。
- Kimi API 属于官方实验接口；调用前读取当前实例 `/openapi.json`，检查工作区注册、会话创建及过滤所需的参数能力，不以固定版本号代替能力判断。
- 仅连接实例文件中 IP 为 loopback 的服务；每个 Rust API 请求建立 TCP 连接后，先通过 Windows 连接表核对实际对端连接属于实例 PID，再核对进程仍存活、映像为 `kimi.exe`、TokenUser SID 与 GitGrove 当前用户一致。持有进程句柄，并排除进程创建时间晚于实例登记的 PID 重用；不要求创建与登记时间相近，因此支持长驻 CLI 稍后开启 Web。
- 验证后的同一个 TCP socket 交给 HTTP/1 客户端发送凭证，每次重连重新校验。没有代理、重定向、连接池或自动重连；无法确认进程、用户或连接归属时停止发送。接口返回 2xx 和非零 PID 本身不作为身份凭据。拒绝关闭默认鉴权的实例。端口取自实例文件，新服务请求系统分配的空闲端口；Kimi 自身会处理抢占后的端口递增。
- `server.token` 仅在 Rust 中读取，用于请求头和发送给系统默认浏览器的 `#token` 登录片段。它不会通过 Tauri 返回给 GitGrove 前端，也不进入日志或错误详情。浏览器交接前，再通过无 HTTP 数据的连接核对原进程和实际端口归属；失效时停止交接。
- 浏览器交接后仍遵循 Kimi 官方 Web 登录协议，浏览器的后续连接与凭证保存由 Kimi 自己处理，不属于上述 Rust 固定 socket 的保护范围。GitGrove 没有代理浏览器连接，也不能保证服务退出后浏览器重连时的端口身份。
- 不提交 prompt，不调用执行、信任、权限或配置写入端点。服务在中性的数据目录启动，不以待打开的仓库作为启动 cwd。
- 前端全局忙碌状态和 Rust 互斥锁防止同一应用中的重复点击；服务连接有界超时。启动失败时结束本次启动的进程，成功后保留服务供浏览器使用。已有服务不会被结束。

参考：[Kimi Server API](https://moonshotai.github.io/kimi-code/zh/reference/server-api.html)、[Kimi Web](https://moonshotai.github.io/kimi-code/zh/guides/web.html)。

## 可重复验收

自动化检查：

```powershell
pnpm -C app lint
pnpm -C app build
pnpm -C app test
node --test .github/scripts/*.test.mjs
cargo check --manifest-path app/src-tauri/Cargo.toml
cargo test --manifest-path app/src-tauri/Cargo.toml --lib
```

通过真实后端调用 GUI（会在目标工具登记项目 / 工作区；不会发送消息）：

```powershell
$env:GITGROVE_AGENT_TARGET = 'C:\existing-project'
$env:GITGROVE_AGENT_TOOL = 'codex' # 或 kimi / kimidesktop / dsh
cargo test --manifest-path app/src-tauri/Cargo.toml --lib agents::tests::desktop_open -- --ignored --exact --nocapture
```

`kimidesktop` 的实机验收只验证 exe 定位与进程启动成功；由于没有完成回执，仍需人工确认 Kimi Code Desktop 窗口中目标工作区已被选中，重复打开复用同一窗口。

macOS 桌面入口验收（同样不发送消息）：

```sh
GITGROVE_AGENT_TARGET='/path/to/existing-project' \
GITGROVE_AGENT_TOOL=codex \
cargo test --manifest-path app/src-tauri/Cargo.toml --lib \
  agents::desktop_tests::desktop_open -- --ignored --exact --nocapture
# 将工具改为 kimidesktop，重复验证应用未运行/已运行与不同 worktree。
```

Mac 自动化测试覆盖逐工具平台开关、Web 后端拒绝、中文/空格/引号/shell 特殊字符的 argv、安装完整性、无效目录及启动器非零退出。UI 验收需确认只有两个桌面入口启用、缺失 Kimi 安装时禁用、安装后重新展开可刷新，以及目标目录实际选中。

隔离 Kimi 冷启动测试：使用新的绝对数据目录和已存在的目标目录。该测试启动真实 Kimi、重复准备同一会话两次，并结束它自己启动的进程；不打开浏览器。测试不自动删除磁盘上的隔离数据。

```powershell
$env:GITGROVE_TEST_KIMI_HOME = 'C:\Temp\gitgrove-kimi-test-home'
$env:GITGROVE_AGENT_TARGET = 'C:\existing-project'
cargo test --manifest-path app/src-tauri/Cargo.toml --lib agents::tests::isolated_cold_start -- --ignored --exact --nocapture
```

协议调用成功不作为 Codex 项目已选中的证明；本地 API 成功不作为 Kimi 页面已进入正确工作区的证明。每次升级适配器或目标工具后，仍需实机确认项目 / 工作树选中、重复打开复用，以及原有 clone / 编辑器 / 终端回归。版本、操作系统和实机结果应随对应 PR 记录。
