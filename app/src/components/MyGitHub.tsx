import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import * as api from "../api";
import { useStore } from "../store";
import type { Project, RepoInfo } from "../types";
import { OpenInMenu } from "./OpenInMenu";
import { ResourceState } from "./ResourceState";

export default function MyGitHub() {
	const s = useStore();
	const [q, setQ] = useState("");
	const [url, setUrl] = useState("");
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState("");
	const list = (s.myRepos ?? []).filter((r) =>
		(r.nameWithOwner + " " + (r.description ?? ""))
			.toLowerCase()
			.includes(q.trim().toLowerCase()),
	);
	const added = (r: RepoInfo) =>
		s.projects.find(
			(p) =>
				p.providerIdentity &&
				(
					p.providerIdentity.owner +
					"/" +
					p.providerIdentity.repo
				).toLowerCase() === r.nameWithOwner.toLowerCase(),
		);
	const openAdded = async (project: Project) => {
		await s.refreshProjects();
		s.setSel({ kind: "project", pid: project.id });
		s.setTab("Overview");
		s.setView("projects");
		s.toast("已打开项目 " + project.name);
	};
	const doClone = async (cloneUrl: string, repo: string) => {
		if (busy) return;
		setBusy(true);
		setError("");
		try {
			const target = await api.checkCloneTarget(repo);
			if (target.exists) {
				s.openDialog({
					kind: "confirm",
					title: "仓库目录已存在",
					message: target.target + "\n可直接将这个目录添加到本地项目。",
					okText: "添加并打开",
					onSubmit: async () =>
						openAdded(await api.addExistingProject(target.target)),
				});
				return;
			}
			s.setCloneProgress("正在克隆 " + repo + "…");
			const project = await api.cloneRepo(cloneUrl, repo);
			setUrl("");
			await openAdded(project);
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
			s.setCloneProgress(null);
		}
	};
	const cloneUrl = () => {
		const match = url
			.trim()
			.match(
				/^(?:https?:\/\/github\.com\/|git@github\.com:)([^/\s]+)\/([^/\s]+?)\/?$/,
			);
		if (!match) {
			setError("请输入有效的 GitHub 仓库地址");
			return;
		}
		const repo = match[2].replace(/\.git$/, "");
		void doClone("https://github.com/" + match[1] + "/" + repo + ".git", repo);
	};
	return (
		<div className="content github-content">
			<div className="account-row">
				{s.auth?.avatarUrl && (
					<img className="avatar" src={s.auth.avatarUrl} alt="" />
				)}
				<div className="grow">
					<strong>{s.auth?.login}</strong>
					<div className="muted">已连接 GitHub</div>
				</div>
				<button
					className="btn"
					disabled={s.reposLoading}
					onClick={() => s.refreshMyRepos()}
				>
					刷新仓库
				</button>
				<button
					className="btn"
					onClick={async () => {
						try {
							await api.logout();
							await s.reloadAuth();
						} catch (e) {
							s.toast(String(e));
						}
					}}
				>
					退出账号
				</button>
			</div>
			<div className="card repo-tools">
				<label className="field">
					<span>查找我的仓库</span>
					<input
						className="input"
						placeholder="搜索名称或描述"
						value={q}
						onChange={(e) => setQ(e.target.value)}
					/>
				</label>
				<form
					className="clone-form"
					onSubmit={(e) => {
						e.preventDefault();
						cloneUrl();
					}}
				>
					<label className="field grow">
						<span>通过地址克隆仓库</span>
						<input
							className="input"
							placeholder="https://github.com/owner/repository"
							disabled={busy}
							value={url}
							onChange={(e) => setUrl(e.target.value)}
						/>
					</label>
					<button
						className="btn primary"
						type="submit"
						disabled={busy || !url.trim()}
					>
						{busy ? "克隆中…" : "克隆并打开"}
					</button>
				</form>
				{s.cloneProgress && (
					<div className="progress-line" role="status">
						{s.cloneProgress}
					</div>
				)}
				{error && (
					<div className="inline-error" role="alert">
						{error}
					</div>
				)}
			</div>
			{s.reposError ? (
				<ResourceState
					error={s.reposError}
					onRetry={() => s.refreshMyRepos()}
				/>
			) : s.reposLoading && !s.myRepos ? (
				<ResourceState loading />
			) : !list.length ? (
				<ResourceState
					title={q ? "没有匹配的仓库" : "没有可显示的仓库"}
					detail={
						q
							? "换个关键词，或通过地址克隆。"
							: "可以通过上方地址克隆公开仓库。"
					}
				/>
			) : (
				<div className="repo-list">
					{list.map((r) => {
						const project = added(r);
						return (
							<div className="repo-row" key={r.nameWithOwner}>
								<div className="grow">
									<strong>{r.nameWithOwner}</strong>
									{r.isPrivate && <span className="priv">私有</span>}
									<div className="sub">
										{r.description || r.language || "暂无描述"}
									</div>
									<div className="sub">
										{r.isFork ? "Fork · " : ""}
										{r.pushedAt ? "更新于 " + relTime(r.pushedAt) : ""}
									</div>
								</div>
								<div className="ops">
									{project && <OpenInMenu path={project.localPath} />}
									<button
										className="btn sm"
										aria-label={"在 GitHub 打开 " + r.nameWithOwner}
										onClick={() =>
											openUrl(r.url).catch((e) => s.toast(String(e)))
										}
									>
										GitHub ↗
									</button>
									{project ? (
										<button
											className="btn sm primary"
											onClick={() => s.openProject(project.id)}
										>
											打开项目
										</button>
									) : (
										<button
											className="btn sm"
											disabled={busy}
											onClick={() =>
												doClone(
													"https://github.com/" + r.nameWithOwner + ".git",
													r.name,
												)
											}
										>
											克隆并打开
										</button>
									)}
								</div>
							</div>
						);
					})}
				</div>
			)}
		</div>
	);
}

export function relTime(iso: string): string {
	const t = new Date(iso).getTime();
	if (Number.isNaN(t)) return iso;
	const s = Math.max(1, Math.floor((Date.now() - t) / 1000));
	if (s < 3600) return `${Math.floor(s / 60)} 分钟前`;
	if (s < 86400) return `${Math.floor(s / 3600)} 小时前`;
	if (s < 86400 * 7) return `${Math.floor(s / 86400)} 天前`;
	if (s < 86400 * 30) return `${Math.floor(s / 86400 / 7)} 周前`;
	return new Date(t).toLocaleDateString();
}
