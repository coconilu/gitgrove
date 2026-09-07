import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import * as api from "../api";
import { CI_LABELS, selectionProject } from "../navigation";
import { useStore } from "../store";
import type { CheckoutInfo, Project } from "../types";
import { addLocalFolder, CiBadge, LinkBadge } from "./CheckoutActions";
import { ResourceState } from "./ResourceState";

export default function Sidebar() {
	const s = useStore();
	const [query, setQuery] = useState("");
	const active = selectionProject(s.projects, s.sel);
	const q = query.trim().toLowerCase();
	const projects = s.projects.filter(
		(p) =>
			!q ||
			(
				p.name +
				" " +
				p.localPath +
				" " +
				p.checkouts.map((c) => c.branch).join(" ")
			)
				.toLowerCase()
				.includes(q),
	);
	const selectCheckout = (c: CheckoutInfo) => {
		s.setSel({ kind: "checkout", cid: c.id });
		s.setView("projects");
	};
	const remove = (p: Project) =>
		s.openDialog({
			kind: "confirm",
			title: "移除项目 " + p.name + "？",
			message:
				p.localPath +
				"\n“仅移除”会保留本地文件。删除文件会将项目目录移入回收站。",
			okText: "移除并删除文件",
			secondaryText: "仅移除，保留文件",
			danger: true,
			onSubmit: async () => {
				await api.removeProject(p.id, true);
				await s.refreshProjects();
				s.toast("项目已移除，文件已入回收站");
			},
			onSecondary: async () => {
				await api.removeProject(p.id, false);
				await s.refreshProjects();
				s.toast("项目已移除，本地文件保留");
			},
		});
	const checkout = (p: Project, c: CheckoutInfo, showProject = false) => (
		<div
			className={
				"checkout-row" +
				((s.sel?.kind === "checkout" && s.sel.cid === c.id) ||
				(s.sel?.kind === "file" && s.sel.co === c.id)
					? " selected"
					: "")
			}
			key={c.id}
		>
			<button
				className="checkout-select"
				title={c.path}
				onClick={() => selectCheckout(c)}
			>
				<span className="checkout-name">
					{showProject && <span className="muted">{p.name} / </span>}
					{c.branch}
				</span>
				<span className="checkout-meta">
					{c.isPrimary ? "主工作区" : "工作树"}
					{c.isLocked ? " · 已锁定" : ""}
				</span>
			</button>
			<div className="checkout-badges">
				<LinkBadge p={p} c={c} link={c.linkedWorkItem} />
				{p.providerIdentity && s.ci[c.id] && (
					<CiBadge status={s.ci[c.id]} compact />
				)}
			</div>
		</div>
	);
	const lanes: Record<string, { p: Project; c: CheckoutInfo }[]> = {};
	for (const p of projects)
		for (const c of p.checkouts) {
			const label =
				s.groupBy === "status"
					? c.linkedWorkItem?.type === "pr"
						? "关联 PR"
						: c.linkedWorkItem
							? "关联 Issue"
							: "未关联任务"
					: CI_LABELS[s.ci[c.id] ?? "unknown"];
			(lanes[label] ??= []).push({ p, c });
		}
	return (
		<aside className="sidebar" aria-label="项目导航">
			<div className="sidebar-heading">
				<strong>工作区</strong>
				<div className="ops">
					<button
						className="btn sm"
						aria-label="刷新项目"
						disabled={s.projectsLoading}
						onClick={() => s.refreshProjects()}
					>
						刷新
					</button>
				</div>
			</div>
			<nav className="side-nav" aria-label="全局导航">
				<button
					className={"nav-chip" + (s.view === "projects" ? " on" : "")}
					aria-current={s.view === "projects" ? "page" : undefined}
					onClick={() => s.setView("projects")}
				>
					本地项目
				</button>
				<button
					className={"nav-chip" + (s.view === "github" ? " on" : "")}
					aria-current={s.view === "github" ? "page" : undefined}
					onClick={() => s.setView("github")}
				>
					GitHub 仓库
				</button>
			</nav>
			<div className="sidebar-search">
				<input
					className="input"
					aria-label="搜索本地项目或分支"
					placeholder="搜索项目或分支"
					value={query}
					onChange={(e) => setQuery(e.target.value)}
				/>
			</div>
			<label className="group-by">
				<span>分组</span>
				<select
					value={s.groupBy}
					onChange={(e) =>
						s.setGroupBy(e.target.value as "repo" | "status" | "ci")
					}
				>
					<option value="repo">按项目</option>
					<option value="status">按任务关联</option>
					<option value="ci">按最近运行</option>
				</select>
			</label>
			<div className="rows">
				{s.projectsError && (
					<ResourceState
						error={s.projectsError}
						onRetry={() => s.refreshProjects()}
					/>
				)}
				{s.projectsLoading && !s.projects.length && <ResourceState loading />}
				{!s.projectsLoading && !s.projectsError && !projects.length && (
					<ResourceState
						title={q ? "没有匹配的项目" : "还没有本地项目"}
						detail={
							q
								? "换个名称或分支名试试。"
								: "添加已有目录，或从 GitHub 克隆仓库。"
						}
					/>
				)}
				{s.groupBy === "repo"
					? projects.map((p) => (
							<div className="proj" key={p.id}>
								<div
									className={
										"project-row" +
										(active?.id === p.id && s.view === "projects"
											? " current"
											: "")
									}
								>
									<button
										className="expand-btn"
										aria-label={
											((s.expanded[p.id] ?? true) ? "收起 " : "展开 ") + p.name
										}
										aria-expanded={s.expanded[p.id] ?? true}
										onClick={() => s.toggleProject(p.id)}
									>
										{(s.expanded[p.id] ?? true) ? "⌄" : "›"}
									</button>
									<button
										className="project-select"
										title={p.localPath}
										onClick={() => s.openProject(p.id)}
									>
										<span className="dot" style={{ background: p.color }} />
										<span>{p.name}</span>
										<span className="count-pill">{p.checkouts.length}</span>
									</button>
									<details className="more-menu project-menu">
										<summary aria-label={"项目操作：" + p.name}>⋯</summary>
										<div
											className="more-popover"
											onClick={(e) =>
												e.currentTarget
													.closest("details")
													?.removeAttribute("open")
											}
										>
											{p.providerIdentity && (
												<button
													onClick={() =>
														openUrl(
															"https://github.com/" +
																p.providerIdentity!.owner +
																"/" +
																p.providerIdentity!.repo,
														).catch((e) => s.toast(String(e)))
													}
												>
													在 GitHub 打开
												</button>
											)}
											<button onClick={() => remove(p)}>移除项目…</button>
										</div>
									</details>
								</div>
								{(s.expanded[p.id] ?? true) &&
									p.checkouts.map((c) => checkout(p, c))}
							</div>
						))
					: Object.entries(lanes).map(([name, entries]) => (
							<section className="proj" key={name}>
								<h3 className="lane-title">
									{name} <span className="count-pill">{entries.length}</span>
								</h3>
								{entries.map(({ p, c }) => checkout(p, c, true))}
							</section>
						))}
			</div>
			<div className="side-foot">
				<button className="btn" onClick={addLocalFolder}>
					添加本地项目
				</button>
				<button className="btn" onClick={() => s.setView("github")}>
					克隆仓库
				</button>
			</div>
		</aside>
	);
}
