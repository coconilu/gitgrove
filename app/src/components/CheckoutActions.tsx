import * as api from "../api";
import { CI_LABELS, worktreePath } from "../navigation";
import { useStore } from "../store";
import type { CheckoutInfo, LinkedWorkItem, Project } from "../types";
import { OpenInMenu } from "./OpenInMenu";

export function CiBadge({
	status,
	compact = false,
}: {
	status: string | null | undefined;
	compact?: boolean;
}) {
	const label = CI_LABELS[status ?? "unknown"] ?? "状态未知";
	return (
		<span
			className={"ci-badge " + (status ?? "unknown")}
			title={"最近 Actions 运行：" + label}
		>
			<span className={"ci " + (status ?? "unknown")} aria-hidden="true" />
			<span>{compact && status === "none" ? "无运行" : label}</span>
		</span>
	);
}
export function LinkBadge({
	p,
	c,
	link,
}: {
	p: Project;
	c: CheckoutInfo;
	link: LinkedWorkItem | null;
}) {
	if (!link) return null;
	return (
		<button
			className={"badge " + (link.type === "issue" ? "b-issue" : "b-pr")}
			title={link.title}
			onClick={(e) => {
				e.stopPropagation();
				const state = useStore.getState();
				state.setSel({ kind: "checkout", cid: c.id });
				state.setTab(link.type === "issue" ? "Issues" : "Pull Requests");
				state.setView("projects");
				state.setWorkItemFocus({
					pid: p.id,
					number: link.number,
					type: link.type,
				});
			}}
		>
			{link.type === "issue" ? "Issue" : "PR"} #{link.number}
		</button>
	);
}
export function abText(ahead: number, behind: number) {
	return (
		<span className="ab">
			{ahead > 0 && <span className="up">领先 {ahead}</span>}
			{behind > 0 && <span className="down">落后 {behind}</span>}
			{!ahead && !behind && <span>已同步</span>}
		</span>
	);
}
export function useCoOps(p: Project) {
	const { openDialog, refreshProjects, toast } = useStore();
	return {
		del: (c: CheckoutInfo) =>
			openDialog({
				kind: "confirm",
				title: "删除工作树 " + c.branch + "？",
				danger: true,
				okText: "移入回收站",
				message:
					c.path +
					"\n将此工作树的文件移入回收站。主仓库和其他工作树不会删除；请先确认没有需要保留的未提交工作。",
				onSubmit: async () => {
					await api.removeWorktree(p.id, c.path, true);
					await refreshProjects();
					toast("工作树已移入回收站");
				},
			}),
		lock: async (c: CheckoutInfo) => {
			try {
				await api.lockWorktree(c.path, !c.isLocked);
				await refreshProjects();
			} catch (e) {
				toast(String(e));
			}
		},
		createPr: (c: CheckoutInfo) =>
			openDialog({
				kind: "prompt",
				title: "创建 Pull Request",
				message: c.branch + " → 仓库默认分支",
				okText: "创建 PR",
				fields: [
					{
						name: "title",
						label: "标题",
						defaultValue: c.linkedWorkItem?.title ?? c.branch,
						required: true,
					},
					{
						name: "body",
						label: "说明",
						type: "textarea",
						defaultValue:
							c.linkedWorkItem?.type === "issue"
								? "Closes #" + c.linkedWorkItem.number
								: "",
					},
				],
				onSubmit: async (_, values) => {
					const url = await api.createPr(
						p.id,
						c.path,
						values.title.trim(),
						values.body,
					);
					toast("PR 已创建：" + url);
					useStore.getState().setTab("Pull Requests");
				},
			}),
	};
}
export function CheckoutActions({ p, c }: { p: Project; c: CheckoutInfo }) {
	const ops = useCoOps(p);
	return (
		<div className="checkout-actions">
			<OpenInMenu key={c.path} path={c.path} />
			{!c.isPrimary && (
				<details className="more-menu">
					<summary className="btn" aria-label={"更多工作树操作：" + c.branch}>
						更多
					</summary>
					<div
						className="more-popover"
						onClick={(e) =>
							e.currentTarget.closest("details")?.removeAttribute("open")
						}
					>
						{p.providerIdentity && (
							<button onClick={() => ops.createPr(c)}>创建 PR</button>
						)}
						<button onClick={() => ops.lock(c)}>
							{c.isLocked ? "解锁工作树" : "锁定工作树"}
						</button>
						<button
							className="danger-text"
							disabled={c.isLocked}
							onClick={() => ops.del(c)}
						>
							删除工作树…
						</button>
					</div>
				</details>
			)}
		</div>
	);
}
export function newWorktree(p: Project) {
	const state = useStore.getState();
	state.openDialog({
		kind: "prompt",
		title: "新建工作树",
		placeholder: "新分支名，例如 feat/my-change",
		message: "为当前项目创建独立工作目录。",
		okText: "创建工作树",
		describe: (v) =>
			"基础分支：" +
			p.defaultBranch +
			"\n目标目录：" +
			(v.trim() ? worktreePath(p.localPath, v.trim()) : "输入分支名后显示"),
		validate: (v) =>
			!v.trim()
				? "请输入分支名"
				: /\s/.test(v.trim())
					? "分支名不能包含空格"
					: null,
		onSubmit: async (v) => {
			const co = await api.createWorktree(
				p.id,
				v.trim(),
				p.defaultBranch,
				true,
				null,
			);
			await state.refreshProjects();
			state.setSel({ kind: "checkout", cid: co.id });
			state.setTab("Worktrees & Branches");
			state.setView("projects");
			state.toast("工作树已创建：" + co.branch);
		},
	});
}
export function addLocalFolder() {
	const state = useStore.getState();
	state.openDialog({
		kind: "prompt",
		title: "添加本地项目",
		placeholder: "现有 Git 仓库的完整路径",
		message:
			"添加已存在的 Git 仓库；关联 GitHub 的项目还可查看 Issues、PR 和 Actions。",
		okText: "添加项目",
		validate: (v) => (v.trim() ? null : "请输入仓库目录"),
		onSubmit: async (v) => {
			const project = await api.addExistingProject(v.trim());
			await state.refreshProjects();
			state.setSel({ kind: "project", pid: project.id });
			state.setTab("Overview");
			state.setView("projects");
			state.toast("已打开项目 " + project.name);
		},
	});
}
