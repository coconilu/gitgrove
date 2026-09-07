import { openUrl } from "@tauri-apps/plugin-opener";
import { marked } from "marked";
import { useEffect, useState } from "react";
import * as api from "../api";
import { ciStatus, TAB_LABELS, TABS } from "../navigation";
import { useStore } from "../store";
import type { CiStatus, IssueInfo, PrInfo, Project, RunInfo } from "../types";
import { abText, CiBadge, LinkBadge, newWorktree } from "./CheckoutActions";
import { relTime } from "./MyGitHub";
import { ResourceState, useResource } from "./ResourceState";

export function ProjectTabs() {
	const { tab, setTab, sel } = useStore();
	return (
		<nav className="tabs" aria-label="项目页面">
			{TABS.map((t) => (
				<button
					key={t}
					className={"tab" + (tab === t ? " on" : "")}
					aria-current={tab === t ? "page" : undefined}
					onClick={() => setTab(t)}
				>
					{TAB_LABELS[t]}
				</button>
			))}
			{sel?.kind === "file" && (
				<span className="preview-indicator">文件预览</span>
			)}
		</nav>
	);
}
export default function ProjectDetail({ p }: { p: Project }) {
	const tab = useStore((s) => s.tab);
	return (
		<div className="content" id="project-content">
			{tab === "Overview" && <Overview p={p} />}
			{tab === "Worktrees & Branches" && <Worktrees p={p} />}
			{tab === "Issues" && <WorkItems p={p} kind="issue" />}
			{tab === "Pull Requests" && <WorkItems p={p} kind="pr" />}
			{tab === "Actions" && <Actions p={p} />}
		</div>
	);
}
function ExternalLink({
	url,
	children,
}: {
	url: string;
	children: React.ReactNode;
}) {
	return (
		<a
			href={url}
			onClick={(e) => {
				e.preventDefault();
				openUrl(url).catch((error) => useStore.getState().toast(String(error)));
			}}
		>
			{children}
		</a>
	);
}
function NoGitHub() {
	return (
		<ResourceState
			title="此项目未关联 GitHub"
			detail="工作树、分支和文件功能仍可使用。GitHub 页面需要仓库具有 github.com 远程地址。"
		/>
	);
}
function Overview({ p }: { p: Project }) {
	const gh = p.providerIdentity;
	const setTab = useStore((s) => s.setTab);
	const issues = useResource(p.id + ":issues", () =>
		gh ? api.listIssues(gh.owner, gh.repo) : Promise.resolve([]),
	);
	const prs = useResource(p.id + ":prs", () =>
		gh ? api.listPrs(gh.owner, gh.repo) : Promise.resolve([]),
	);
	const primary = p.checkouts.find((c) => c.isPrimary);
	const run = useResource<RunInfo | null>(
		p.id + ":overview-run:" + primary?.branch,
		() =>
			gh && primary
				? api.latestRunForBranch(gh.owner, gh.repo, primary.branch)
				: Promise.resolve(null),
	);
	const readme = useResource<string | null>(p.id + ":readme", async () => {
		const files = await api.readDir(p.localPath);
		const name = files.find(
			(f) => !f.isDir && f.name.toLowerCase() === "readme.md",
		)?.name;
		if (!name) return null;
		const file = await api.readFilePreview(p.localPath + "/" + name);
		return file.isBinary ? null : file.text;
	});
	return (
		<>
			<div className="page-heading">
				<div>
					<h2>{gh ? gh.owner + "/" + gh.repo : p.name}</h2>
					<p className="muted path-text">{p.localPath}</p>
				</div>
				<button
					className="btn"
					onClick={() => {
						issues.reload();
						prs.reload();
						run.reload();
						readme.reload();
					}}
				>
					刷新概览
				</button>
			</div>
			<div className="stat-row">
				<button className="stat" onClick={() => setTab("Worktrees & Branches")}>
					<b>{p.checkouts.length}</b>
					<span>工作区</span>
				</button>
				<button
					className="stat"
					disabled={!gh}
					onClick={() => setTab("Issues")}
				>
					<b>
						{!gh || issues.error
							? "—"
							: issues.loading
								? "…"
								: issues.data?.filter((i) => i.state === "open").length}
					</b>
					<span>{issues.error ? "Issues 读取失败" : "待处理 Issues"}</span>
				</button>
				<button
					className="stat"
					disabled={!gh}
					onClick={() => setTab("Pull Requests")}
				>
					<b>
						{!gh || prs.error
							? "—"
							: prs.loading
								? "…"
								: prs.data?.filter((i) => i.state === "open").length}
					</b>
					<span>{prs.error ? "PR 读取失败" : "开放的 PR"}</span>
				</button>
				<button
					className="stat"
					disabled={!gh}
					onClick={() => setTab("Actions")}
				>
					<CiBadge
						status={
							run.error ? "unknown" : run.loading ? null : ciStatus(run.data)
						}
					/>
					<span>{run.loading ? "读取最近运行…" : "最近 Actions"}</span>
				</button>
			</div>
			{(issues.error || prs.error || run.error) && (
				<div className="inline-error" role="alert">
					部分 GitHub 信息读取失败，显示的“—”不代表数量为零。可刷新概览重试。
				</div>
			)}
			<div className="card next-actions">
				<strong>继续项目工作</strong>
				<p className="muted">
					通过顶部“在…中打开”进入 Codex、Kimi
					Code、编辑器或终端；选择工作树后使用对应目录。
				</p>
				<div className="ops">
					<button
						className="btn primary"
						onClick={() => setTab("Worktrees & Branches")}
					>
						管理工作树
					</button>
					{gh && (
						<button className="btn" onClick={() => setTab("Issues")}>
							处理 Issue
						</button>
					)}
					{gh && (
						<ExternalLink
							url={"https://github.com/" + gh.owner + "/" + gh.repo}
						>
							在 GitHub 打开 ↗
						</ExternalLink>
					)}
				</div>
			</div>
			{readme.loading || readme.error ? (
				<ResourceState
					loading={readme.loading}
					error={readme.error}
					onRetry={readme.reload}
				/>
			) : readme.data ? (
				<div
					className="card md"
					// biome-ignore lint/security/noDangerouslySetInnerHtml: 现有本地 Markdown 预览行为
					dangerouslySetInnerHTML={{
						__html: marked.parse(readme.data) as string,
					}}
				/>
			) : (
				<p className="muted">
					项目尚未提供 README.md，可通过文件面板浏览源码。
				</p>
			)}
		</>
	);
}
function Worktrees({ p }: { p: Project }) {
	const { ci, setSel, setView, sel, refreshProjects, toast } = useStore();
	const branches = useResource(p.id + ":branches", () =>
		api.listBranches(p.id),
	);
	const [busy, setBusy] = useState("");
	const [error, setError] = useState("");
	const enter = (cid: string) => {
		setSel({ kind: "checkout", cid });
		setView("projects");
	};
	return (
		<>
			<div className="page-heading">
				<div>
					<h2>工作树与分支</h2>
					<p className="muted">每个工作树有独立目录；选中后使用顶部操作。</p>
				</div>
				<div className="ops">
					<button
						className="btn"
						onClick={() => {
							branches.reload();
							void refreshProjects();
						}}
					>
						刷新
					</button>
					<button className="btn primary" onClick={() => newWorktree(p)}>
						新建工作树
					</button>
				</div>
			</div>
			{error && (
				<div className="inline-error" role="alert">
					{error}
				</div>
			)}
			<div className="worktree-list">
				{p.checkouts.map((c) => (
					<div
						className={
							"worktree-row" +
							(sel?.kind === "checkout" && sel.cid === c.id ? " selected" : "")
						}
						key={c.id}
					>
						<div className="grow">
							<strong>{c.branch}</strong>{" "}
							<span className="badge b-primary">
								{c.isPrimary ? "主工作区" : "工作树"}
							</span>
							{c.isLocked && <span className="priv">已锁定</span>}
							<div className="path-text muted">{c.path}</div>
							<div className="worktree-meta">
								{abText(c.ahead, c.behind)}
								<LinkBadge p={p} c={c} link={c.linkedWorkItem} />
								{p.providerIdentity && <CiBadge status={ci[c.id]} />}
							</div>
						</div>
						<button className="btn" onClick={() => enter(c.id)}>
							{sel?.kind === "checkout" && sel.cid === c.id
								? "当前工作树"
								: "进入工作树"}
						</button>
					</div>
				))}
			</div>
			<h3 className="section-title">仓库分支</h3>
			{branches.loading || branches.error ? (
				<ResourceState
					loading={branches.loading}
					error={branches.error}
					onRetry={branches.reload}
				/>
			) : !branches.data?.length ? (
				<ResourceState
					title="没有可显示的分支"
					detail="新仓库可能还没有提交。"
				/>
			) : (
				<div className="branch-list">
					{branches.data.map((b) => {
						const checkout = p.checkouts.find(
							(c) => !b.remote && c.branch === b.name,
						);
						return (
							<div
								className="branch-row"
								key={(b.remote ? "r:" : "l:") + b.name}
							>
								<div className="grow">
									<strong>{b.name}</strong>{" "}
									<span className="muted">{b.remote ? "远程" : "本地"}</span>
									<div className="sub" title={b.subject}>
										{b.subject || "暂无提交说明"}
										{b.lastCommitTs
											? " · " +
												relTime(new Date(b.lastCommitTs * 1000).toISOString())
											: ""}
									</div>
								</div>
								{abText(b.ahead, b.behind)}
								{!b.remote && (
									<button
										className="btn sm"
										disabled={Boolean(busy)}
										onClick={async () => {
											if (checkout) {
												enter(checkout.id);
												return;
											}
											setBusy(b.name);
											setError("");
											try {
												const co = await api.createWorktree(
													p.id,
													b.name,
													null,
													false,
													null,
												);
												await refreshProjects();
												enter(co.id);
												branches.reload();
												toast("工作树已创建");
											} catch (e) {
												setError(String(e));
											} finally {
												setBusy("");
											}
										}}
									>
										{busy === b.name
											? "创建中…"
											: checkout
												? "进入已有工作树"
												: "创建工作树"}
									</button>
								)}
							</div>
						);
					})}
				</div>
			)}
		</>
	);
}
function WorkItems({ p, kind }: { p: Project; kind: "issue" | "pr" }) {
	const gh = p.providerIdentity;
	const s = useStore();
	const items = useResource<(IssueInfo | PrInfo)[]>(p.id + ":" + kind, () =>
		!gh
			? Promise.resolve([])
			: kind === "issue"
				? api.listIssues(gh.owner, gh.repo)
				: api.listPrs(gh.owner, gh.repo),
	);
	const [filter, setFilter] = useState("open");
	const [q, setQ] = useState("");
	const [busy, setBusy] = useState<number | null>(null);
	const [error, setError] = useState("");
	const focus =
		s.workItemFocus?.pid === p.id && s.workItemFocus.type === kind
			? s.workItemFocus
			: null;
	const prs = kind === "pr" ? ((items.data ?? []) as PrInfo[]) : [];
	const checks = useResource<Record<number, CiStatus>>(
		p.id +
			":" +
			kind +
			":runs:" +
			prs
				.slice(0, 20)
				.map((pr) => pr.number + pr.headRef)
				.join(","),
		async () => {
			const result: Record<number, CiStatus> = {};
			if (!gh) return result;
			for (let i = 0; i < Math.min(prs.length, 20); i += 4)
				await Promise.all(
					prs.slice(i, i + 4).map(async (pr) => {
						try {
							result[pr.number] = ciStatus(
								await api.latestRunForBranch(gh.owner, gh.repo, pr.headRef),
							);
						} catch {
							result[pr.number] = "unknown";
						}
					}),
				);
			return result;
		},
	);
	useEffect(() => {
		if (!focus || !items.data) return;
		setFilter("all");
		setQ("");
		const frame = requestAnimationFrame(() =>
			document
				.getElementById("work-item-" + kind + "-" + focus.number)
				?.scrollIntoView({ block: "nearest" }),
		);
		return () => cancelAnimationFrame(frame);
	}, [focus, kind, items.data]);
	if (!gh) return <NoGitHub />;
	const list = (items.data ?? []).filter(
		(i) =>
			(filter === "all" ||
				(filter === "open" ? i.state === "open" : i.state !== "open")) &&
			(i.title + " " + i.number).toLowerCase().includes(q.toLowerCase()),
	);
	return (
		<>
			<div className="page-heading">
				<div>
					<h2>{kind === "issue" ? "Issues" : "Pull Requests"}</h2>
					<p className="muted">
						{kind === "issue"
							? "从任务创建工作树，或回到已关联的工作。"
							: "查看变更，进入独立工作树审查。"}
					</p>
				</div>
				<button
					className="btn"
					onClick={() => {
						items.reload();
						checks.reload();
					}}
				>
					刷新
				</button>
			</div>
			<div className="list-tools">
				<input
					className="input"
					aria-label="搜索标题或编号"
					placeholder="搜索标题或编号"
					value={q}
					onChange={(e) => {
						s.setWorkItemFocus(null);
						setQ(e.target.value);
					}}
				/>
				<select
					className="input"
					aria-label="任务状态"
					value={filter}
					onChange={(e) => {
						s.setWorkItemFocus(null);
						setFilter(e.target.value);
					}}
				>
					<option value="open">开放</option>
					<option value="closed">已关闭 / 合并</option>
					<option value="all">全部</option>
				</select>
			</div>
			{error && (
				<div className="inline-error" role="alert">
					{error}
				</div>
			)}
			{items.loading || items.error ? (
				<ResourceState
					loading={items.loading}
					error={items.error}
					onRetry={items.reload}
				/>
			) : !list.length ? (
				<ResourceState
					title={
						q
							? "没有匹配的结果"
							: "当前没有" + (kind === "issue" ? " Issue" : " PR")
					}
					detail="可切换状态查看历史记录，或到 GitHub 查看。"
					action={
						<ExternalLink
							url={
								"https://github.com/" +
								gh.owner +
								"/" +
								gh.repo +
								(kind === "issue" ? "/issues" : "/pulls")
							}
						>
							前往 GitHub ↗
						</ExternalLink>
					}
				/>
			) : (
				<div className="work-item-list">
					{list.map((item) => {
						const co = p.checkouts.find(
							(c) =>
								c.linkedWorkItem?.type === kind &&
								c.linkedWorkItem.number === item.number,
						);
						const pr = kind === "pr" ? (item as PrInfo) : null;
						return (
							<article
								id={"work-item-" + kind + "-" + item.number}
								className={
									"work-item" +
									(focus?.number === item.number ? " linked-focus" : "")
								}
								key={item.number}
							>
								<div className="grow">
									<div className="work-item-title">
										<span
											className={
												"item-state " +
												(item.state === "open" ? "open" : "closed")
											}
										>
											{pr?.merged
												? "已合并"
												: pr?.draft
													? "草稿"
													: item.state === "open"
														? "开放"
														: "已关闭"}
										</span>
										<ExternalLink url={item.url}>{item.title}</ExternalLink>
									</div>
									<div className="worktree-meta">
										<span className="muted">#{item.number}</span>
										{pr ? (
											<>
												<span className="muted">{pr.headRef}</span>
												<CiBadge
													status={checks.data?.[item.number] ?? "unknown"}
												/>
											</>
										) : (
											(item as IssueInfo).labels.map((l) => (
												<span className="label-chip" key={l.name}>
													{l.name}
												</span>
											))
										)}
										{co && (
											<span className="badge b-primary">
												工作树：{co.branch}
											</span>
										)}
									</div>
								</div>
								{(co || item.state === "open") && (
									<button
										className="btn sm"
										disabled={busy !== null}
										onClick={async () => {
											setError("");
											if (co) {
												s.setSel({ kind: "checkout", cid: co.id });
												s.setTab("Worktrees & Branches");
												return;
											}
											setBusy(item.number);
											try {
												const created =
													kind === "issue"
														? await api.spawnIssueWorktree(
																p.id,
																item.number,
																item.title,
															)
														: await api.spawnPrWorktree(p.id, item.number);
												await s.refreshProjects();
												s.setSel({ kind: "checkout", cid: created.id });
												s.setTab("Worktrees & Branches");
												s.setView("projects");
												s.toast("已创建并打开工作树 " + created.branch);
											} catch (e) {
												setError(String(e));
											} finally {
												setBusy(null);
											}
										}}
									>
										{busy === item.number
											? "创建中…"
											: co
												? "进入工作树"
												: kind === "pr"
													? "创建审查工作树"
													: "创建工作树"}
									</button>
								)}
							</article>
						);
					})}
				</div>
			)}
		</>
	);
}
function Actions({ p }: { p: Project }) {
	const gh = p.providerIdentity;
	const s = useStore();
	const workflows = useResource(p.id + ":workflows", () =>
		gh ? api.listWorkflows(gh.owner, gh.repo) : Promise.resolve([]),
	);
	const [selected, setSelected] = useState<number | null>(null);
	const [busy, setBusy] = useState<number | null>(null);
	const [error, setError] = useState("");
	const workflow =
		workflows.data?.find((w) => w.id === selected) ?? workflows.data?.[0];
	const details = useResource(p.id + ":workflow-details:" + workflow?.id, () =>
		gh && workflow
			? api.workflowDetails(gh.owner, gh.repo, workflow.id)
			: Promise.resolve(null),
	);
	const runs = useResource(p.id + ":workflow-runs:" + workflow?.id, () =>
		gh && workflow
			? api.listRuns(gh.owner, gh.repo, workflow.id)
			: Promise.resolve([]),
	);
	if (!gh) return <NoGitHub />;
	const dispatch = () => {
		if (!workflow || !details.data?.dispatch) return;
		const fields = [
			{
				name: "ref",
				label: "运行分支或标签",
				defaultValue: details.data.defaultBranch,
				required: true,
			},
			...details.data.inputs.map((input) => ({
				name: "input:" + input.name,
				label: input.description || input.name,
				type: (input.kind === "choice" || input.kind === "boolean"
					? "select"
					: input.kind === "number"
						? "number"
						: "text") as "select" | "number" | "text",
				defaultValue:
					input.defaultValue || (input.kind === "boolean" ? "false" : ""),
				required: input.required,
				options: input.kind === "boolean" ? ["false", "true"] : input.options,
			})),
		];
		s.openDialog({
			kind: "prompt",
			title: "运行 " + workflow.name,
			fields,
			okText: "运行流程",
			message:
				"将为 " +
				gh.owner +
				"/" +
				gh.repo +
				" 触发一次 GitHub Actions 运行。参数来自默认分支的流程配置。",
			validate: (_, v) => (!v.ref.trim() ? "请输入分支或标签" : null),
			onSubmit: async (_, values) => {
				const inputs = Object.fromEntries(
					Object.entries(values)
						.filter(([key, value]) => key.startsWith("input:") && value !== "")
						.map(([key, value]) => [key.slice(6), value]),
				);
				await api.dispatchWorkflow(
					gh.owner,
					gh.repo,
					workflow.id,
					values.ref.trim(),
					inputs,
				);
				runs.reload();
				s.toast("已请求运行流程；可刷新查看进度");
			},
		});
	};
	return (
		<>
			<div className="page-heading">
				<div>
					<h2>Actions</h2>
					<p className="muted">
						查看运行结果和日志，按需触发支持手动运行的流程。
					</p>
				</div>
				<button
					className="btn"
					onClick={() => {
						workflows.reload();
						runs.reload();
						details.reload();
					}}
				>
					刷新
				</button>
			</div>
			{workflows.loading || workflows.error ? (
				<ResourceState
					loading={workflows.loading}
					error={workflows.error}
					onRetry={workflows.reload}
				/>
			) : !workflows.data?.length ? (
				<ResourceState
					title="尚未配置工作流"
					detail="在 GitHub 仓库中配置 Actions 后，会显示在这里。"
				/>
			) : (
				<div className="actions-layout">
					<nav className="workflow-nav" aria-label="工作流">
						{workflows.data.map((w) => (
							<button
								key={w.id}
								className={"wf" + (workflow?.id === w.id ? " on" : "")}
								aria-current={workflow?.id === w.id ? "page" : undefined}
								onClick={() => {
									setSelected(w.id);
									setError("");
								}}
							>
								{w.name}
							</button>
						))}
					</nav>
					<section className="workflow-detail">
						<div className="workflow-heading">
							<strong>{workflow?.name}</strong>
							<button
								className="btn primary"
								disabled={
									!details.data?.dispatch ||
									workflow?.state !== "active" ||
									details.loading
								}
								onClick={dispatch}
							>
								运行流程
							</button>
						</div>
						{details.error ? (
							<ResourceState error={details.error} onRetry={details.reload} />
						) : (
							<p className="muted">
								{details.loading
									? "读取运行配置…"
									: workflow?.state !== "active"
										? "该流程已停用"
										: details.data?.dispatch
											? "默认分支：" + details.data.defaultBranch
											: "此流程由事件触发，不支持手动运行"}
							</p>
						)}
						{error && (
							<div className="inline-error" role="alert">
								{error}
							</div>
						)}
						{runs.loading || runs.error ? (
							<ResourceState
								loading={runs.loading}
								error={runs.error}
								onRetry={runs.reload}
							/>
						) : !runs.data?.length ? (
							<ResourceState
								title="暂无运行记录"
								detail="流程运行后，会在这里显示结果。"
							/>
						) : (
							<div className="run-list">
								{runs.data.map((r) => (
									<div className="workflow-run" key={r.id}>
										<div className="grow">
											<strong>
												#{r.runNumber} · {r.branch}
											</strong>
											<div className="sub">
												{r.actor} · {relTime(r.createdAt)}
											</div>
											<CiBadge status={ciStatus(r)} />
										</div>
										<div className="ops">
											<ExternalLink url={r.url}>日志 ↗</ExternalLink>
											{(r.status !== "completed" ||
												r.conclusion === "failure") && (
												<button
													className="btn sm"
													disabled={busy !== null}
													onClick={async () => {
														setBusy(r.id);
														setError("");
														try {
															if (r.status === "completed")
																await api.rerunRun(gh.owner, gh.repo, r.id);
															else await api.cancelRun(gh.owner, gh.repo, r.id);
															runs.reload();
															s.toast("操作已提交");
														} catch (e) {
															setError(String(e));
														} finally {
															setBusy(null);
														}
													}}
												>
													{busy === r.id
														? "处理中…"
														: r.status === "completed"
															? "重新运行"
															: "取消运行"}
												</button>
											)}
										</div>
									</div>
								))}
							</div>
						)}
					</section>
				</div>
			)}
		</>
	);
}
