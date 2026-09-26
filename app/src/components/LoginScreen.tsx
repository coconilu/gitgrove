import { useState } from "react";
import * as api from "../api";
import { useStore } from "../store";
import { ExternalLink } from "./ProjectDetail";

// 经典 PAT 创建页，预选 repo + workflow scope（与上方说明一致），描述占位
const NEW_TOKEN_URL =
	"https://github.com/settings/tokens/new?scopes=repo,workflow&description=GitGrove";

export default function LoginScreen() {
	const { reloadAuth, toast } = useStore();
	const [pat, setPat] = useState("");
	const [busy, setBusy] = useState(false);

	const tryGh = async () => {
		setBusy(true);
		await reloadAuth();
		setBusy(false);
		if (!useStore.getState().auth?.loggedIn)
			toast("未检测到 gh CLI 登录态，请先用 gh auth login 或使用 PAT");
	};

	const doPat = async () => {
		if (!pat.trim()) return;
		setBusy(true);
		try {
			await api.loginPat(pat.trim());
			await reloadAuth();
		} catch (e) {
			toast(`登录失败: ${e}`);
		} finally {
			setBusy(false);
		}
	};

	return (
		<div className="login-wrap">
			<div className="card login-card">
				<div
					style={{
						display: "flex",
						alignItems: "center",
						gap: 10,
						marginBottom: 14,
					}}
				>
					<img
						src={
							new URL("../../src-tauri/icons/32x32.png", import.meta.url).href
						}
						width={32}
						height={32}
						alt="GitGrove"
					/>
					<h2 style={{ margin: 0 }}>登录 GitHub</h2>
				</div>
				<p
					className="msg"
					style={{ color: "var(--muted)", fontSize: 12, marginBottom: 12 }}
				>
					已安装 gh CLI 并完成 gh auth login 时，可直接复用其登录态；token
					会存入系统凭据管理器，不会进入前端上下文。
				</p>
				<button
					className="btn primary"
					style={{ width: "100%", padding: 8 }}
					disabled={busy}
					onClick={tryGh}
				>
					使用 gh CLI 登录
				</button>
				<div
					style={{
						borderTop: "1px solid var(--border-soft)",
						margin: "16px 0",
					}}
				/>
				<p style={{ color: "var(--muted)", fontSize: 12, marginBottom: 8 }}>
					或粘贴 Personal Access Token（需 repo + workflow 权限）：{" "}
					<ExternalLink url={NEW_TOKEN_URL}>去 GitHub 创建 ↗</ExternalLink>
				</p>
				<input
					className="input"
					type="password"
					placeholder="ghp_… / github_pat_…"
					value={pat}
					onChange={(e) => setPat(e.target.value)}
					onKeyDown={(e) => e.key === "Enter" && doPat()}
				/>
				<button
					className="btn"
					style={{ width: "100%", padding: 8, marginTop: 10 }}
					disabled={busy || !pat.trim()}
					onClick={doPat}
				>
					使用 PAT 登录
				</button>
			</div>
		</div>
	);
}
