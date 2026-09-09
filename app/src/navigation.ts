import type {
	CheckoutInfo,
	CiStatus,
	Project,
	RunInfo,
	Selection,
} from "./types";

export const TABS = [
	"Overview",
	"Worktrees & Branches",
	"Issues",
	"Pull Requests",
	"Projects",
	"Actions",
] as const;
export const TAB_LABELS: Record<string, string> = {
	Overview: "概览",
	"Worktrees & Branches": "工作树与分支",
	Issues: "Issues",
	"Pull Requests": "Pull Requests",
	Projects: "Projects",
	Actions: "Actions",
};
export type Visit = { sel: Selection; tab: string };
export type Navigation = {
	sel: Selection | null;
	tab: string;
	visits: Record<string, Visit>;
};

export function selectionProject(
	projects: Project[],
	selection: Selection | null,
) {
	if (!selection) return undefined;
	if (selection.kind === "project")
		return projects.find((p) => p.id === selection.pid);
	const cid = selection.kind === "file" ? selection.co : selection.cid;
	return projects.find((p) => p.checkouts.some((c) => c.id === cid));
}

export function navigate(
	projects: Project[],
	state: Navigation,
	sel: Selection | null,
): Navigation {
	const previous = selectionProject(projects, state.sel);
	const next = selectionProject(projects, sel);
	const visits = { ...state.visits };
	if (previous && state.sel)
		visits[previous.id] = { sel: state.sel, tab: state.tab };
	const tab =
		next && next.id !== previous?.id
			? (visits[next.id]?.tab ?? "Overview")
			: state.tab;
	if (next && sel) visits[next.id] = { sel, tab };
	return { sel, tab, visits };
}

export function projectVisit(
	projects: Project[],
	visits: Record<string, Visit>,
	pid: string,
): Visit {
	const saved = visits[pid];
	if (saved && selectionProject(projects, saved.sel)?.id === pid) return saved;
	return { sel: { kind: "project", pid }, tab: saved?.tab ?? "Overview" };
}

export function previewParent(sel: Selection | null): Selection | null {
	return sel?.kind === "file" ? { kind: "checkout", cid: sel.co } : sel;
}

export function ciStatus(run: RunInfo | null): CiStatus {
	if (!run) return "none";
	if (run.status !== "completed") return "run";
	if (run.conclusion === "success") return "ok";
	if (
		["failure", "timed_out", "action_required", "startup_failure"].includes(
			run.conclusion ?? "",
		)
	)
		return "fail";
	if (run.conclusion === "cancelled") return "cancelled";
	if (run.conclusion === "skipped") return "skipped";
	return "unknown";
}

export const CI_LABELS: Record<string, string> = {
	ok: "运行通过",
	run: "运行中",
	fail: "运行失败",
	none: "暂无运行",
	cancelled: "已取消",
	skipped: "已跳过",
	unknown: "状态未知",
};

export function worktreePath(localPath: string, branch: string) {
	const clean = localPath.replaceAll("\\", "/").replace(/\/$/, "");
	const slug =
		branch
			.replace(/[^a-zA-Z0-9]+/g, "-")
			.toLowerCase()
			.replace(/^-|-$/g, "")
			.slice(0, 40) || "wt";
	return clean + ".worktrees/" + slug;
}

export function findCheckout(
	projects: Project[],
	cid: string,
): { p: Project; c: CheckoutInfo } | null {
	for (const p of projects)
		for (const c of p.checkouts) if (c.id === cid) return { p, c };
	return null;
}
export function activeCheckout(
	projects: Project[],
	sel: Selection | null,
): { p: Project; c: CheckoutInfo } | null {
	if (!sel) return null;
	if (sel.kind === "checkout") return findCheckout(projects, sel.cid);
	if (sel.kind === "file") return findCheckout(projects, sel.co);
	const p = projects.find((x) => x.id === sel.pid);
	const c = p?.checkouts.find((x) => x.isPrimary);
	return p && c ? { p, c } : null;
}
