// PM 纯逻辑：过滤、分组、里程碑聚合、截止日计算、加载编排。不依赖 React，供 node --test 直接测。
import type {
	PmItem,
	PmMilestoneWithStats,
	PmStatusDef,
	PmSyncResult,
} from "../../types";

/** 与后端 model.rs DEFAULT_STATUSES 对齐；pm_list_statuses 不可用时的回退 */
export const DEFAULT_STATUSES: PmStatusDef[] = [
	{ id: "backlog", name: "Backlog" },
	{ id: "todo", name: "Todo" },
	{ id: "doing", name: "Doing" },
	{ id: "done", name: "Done" },
];

export const PRIORITIES = ["none", "low", "medium", "high", "urgent"] as const;

export const PRIORITY_LABELS: Record<string, string> = {
	none: "无",
	low: "低",
	medium: "中",
	high: "高",
	urgent: "紧急",
};

export interface BoardFilter {
	milestoneId: string | null;
	repoPath: string | null;
	label: string | null;
	priority: string | null;
	search: string;
}

export const EMPTY_FILTER: BoardFilter = {
	milestoneId: null,
	repoPath: null,
	label: null,
	priority: null,
	search: "",
};

export function filterItems(items: PmItem[], f: BoardFilter): PmItem[] {
	const q = f.search.trim().toLowerCase();
	return items.filter(
		(i) =>
			(!f.milestoneId || i.milestoneId === f.milestoneId) &&
			(!f.repoPath || i.repoPath === f.repoPath) &&
			(!f.label || i.labels.includes(f.label)) &&
			(!f.priority || i.priority === f.priority) &&
			(!q ||
				i.title.toLowerCase().includes(q) ||
				i.body.toLowerCase().includes(q)),
	);
}

/** 按看板列分组，列内按 order 字符串序排列；未知状态的 item 不进任何列 */
export function groupByStatus(
	items: PmItem[],
	statuses: PmStatusDef[],
): Map<string, PmItem[]> {
	const groups = new Map<string, PmItem[]>(statuses.map((s) => [s.id, []]));
	for (const item of items) groups.get(item.status)?.push(item);
	for (const list of groups.values())
		list.sort((a, b) => (a.order < b.order ? -1 : a.order > b.order ? 1 : 0));
	return groups;
}

/** 后端约定：done = 最后一个看板列 */
export function doneStatusId(statuses: PmStatusDef[]): string {
	return statuses[statuses.length - 1]?.id ?? "done";
}

/** 仓库路径取末段作为卡片/过滤条上的短名 */
export function repoName(repoPath: string): string {
	const clean = repoPath.replaceAll("\\", "/").replace(/\/$/, "");
	return clean.split("/").pop() || clean;
}

export interface GithubRef {
	owner: string;
	repo: string;
	number: number;
}

/**
 * 解析后端约定的 githubRef（`owner/repo#number`，见 sync.rs）。
 * 格式不符返回 null（老数据 / 手填字段容错）。
 */
export function parseGithubRef(ref: string): GithubRef | null {
	const m = /^([^/\s#]+)\/([^/\s#]+)#(\d+)$/.exec(ref);
	if (!m) return null;
	const number = Number(m[3]);
	return { owner: m[1], repo: m[2], number };
}

/** githubRef 对应的 issue 页面链接；解析失败返回 null（不渲染徽标链接） */
export function issueUrl(ref: string): string | null {
	const parsed = parseGithubRef(ref);
	return parsed
		? `https://github.com/${parsed.owner}/${parsed.repo}/issues/${parsed.number}`
		: null;
}

/** 本地日期 YYYY-MM-DD（不用 toISOString，避免时区偏移） */
export function todayString(d: Date = new Date()): string {
	const p = (n: number) => String(n).padStart(2, "0");
	return d.getFullYear() + "-" + p(d.getMonth() + 1) + "-" + p(d.getDate());
}

/** 两个 YYYY-MM-DD 之间相差的天数（b - a） */
export function dayDiff(a: string, b: string): number {
	return Math.round((Date.parse(b) - Date.parse(a)) / 86400000);
}

export interface DueInfo {
	/** MM-DD 短日期 */
	short: string;
	/** 倒计时文案：今天到期 / 还剩 N 天 / 已逾期 N 天 */
	text: string;
	overdue: boolean;
	dueToday: boolean;
}

export function dueInfo(dueDate: string, today: string): DueInfo {
	const days = dayDiff(today, dueDate);
	return {
		short: dueDate.slice(5),
		text:
			days === 0
				? "今天到期"
				: days > 0
					? "还剩 " + days + " 天"
					: "已逾期 " + -days + " 天",
		overdue: days < 0,
		dueToday: days === 0,
	};
}

export interface MilestoneProgress {
	total: number;
	done: number;
	doing: number;
	/** 其余列（backlog/todo/自定义）合计 */
	rest: number;
	pct: number;
}

export function milestoneProgress(
	items: PmItem[],
	milestoneId: string,
	doneId: string,
): MilestoneProgress {
	let done = 0;
	let doing = 0;
	let rest = 0;
	for (const i of items) {
		if (i.milestoneId !== milestoneId) continue;
		if (i.status === doneId) done++;
		else if (i.status === "doing") doing++;
		else rest++;
	}
	const total = done + doing + rest;
	return {
		total,
		done,
		doing,
		rest,
		pct: total ? Math.round((done / total) * 100) : 0,
	};
}

export interface FocusSummary {
	doing: PmItem[];
	dueToday: PmItem[];
}

/** 「今日聚焦」：全部 doing + 今日到期（不含已完成列） */
export function focusSummary(
	items: PmItem[],
	today: string,
	doneId: string,
): FocusSummary {
	const doing = items.filter((i) => i.status === "doing");
	const dueToday = items.filter(
		(i) => i.status !== doneId && i.dueDate === today,
	);
	return { doing, dueToday };
}

/** 里程碑列表排序：开放的在前，按截止日升序（无截止排最后），再按创建时间 */
export function sortMilestones(
	list: PmMilestoneWithStats[],
): PmMilestoneWithStats[] {
	return [...list].sort((a, b) => {
		if ((a.status === "closed") !== (b.status === "closed"))
			return a.status === "closed" ? 1 : -1;
		if ((a.dueDate === null) !== (b.dueDate === null))
			return a.dueDate === null ? 1 : -1;
		if (a.dueDate && b.dueDate && a.dueDate !== b.dueDate)
			return a.dueDate < b.dueDate ? -1 : 1;
		return a.createdAt - b.createdAt;
	});
}

export interface MoveTarget {
	toStatus: string;
	beforeItemId: string | null;
}

/**
 * 由拖拽落点计算移动参数：overId 是列 id（排到列尾）或 item id（插到它前面）。
 * 落点是自身或找不到目标时返回 null（无需移动）。
 */
export function computeMove(
	items: PmItem[],
	statuses: PmStatusDef[],
	activeId: string,
	overId: string | null,
): MoveTarget | null {
	const active = items.find((i) => i.id === activeId);
	if (!active) return null;
	if (overId === null) return null;
	if (statuses.some((s) => s.id === overId)) {
		// 落在列容器上：同列尾部不动，跨列排到尾部
		if (active.status === overId) return null;
		return { toStatus: overId, beforeItemId: null };
	}
	const over = items.find((i) => i.id === overId);
	if (!over || over.id === active.id) return null;
	return { toStatus: over.status, beforeItemId: over.id };
}

// ---- 数据加载编排（#61）：本地数据先渲染，GitHub 同步后台化 ----

/** 前端同步超时兜底：后端网络有界（connect 5s / 请求 10s），此处再兜住 gh CLI、
 * keyring 等无法覆盖的悬挂——超时后放弃等待当次同步，看板不受影响 */
export const PM_SYNC_TIMEOUT_MS = 15_000;

/**
 * 后台刷新与本地乐观变更的竞态闸门（#77）：
 * 拖拽的乐观更新已改本地顺序、服务端尚未回填时，到达的整表刷新（SWR 后台
 * 刷新 / 同步后重载）一律扣下不应用，避免拖动中的卡片被弹回旧位置；
 * begin/end 配对归零后若扣下过刷新，调用方重新拉取本地数据收口。
 */
export function createRefreshGate() {
	let depth = 0;
	let held = false;
	return {
		/** 乐观变更开始；可嵌套计数（如拖拽中追加的 manualLock 请求） */
		begin(): void {
			depth++;
		},
		/** 乐观变更结束：期间有刷新被扣下则返回 true（调用方重载收口） */
		end(): boolean {
			depth = Math.max(0, depth - 1);
			if (depth > 0) return false;
			const had = held;
			held = false;
			return had;
		},
		/** 刷新结果到达：true 立即应用；false 表示变更进行中，已扣下 */
		offer(): boolean {
			if (depth > 0) {
				held = true;
				return false;
			}
			return true;
		},
	};
}

export interface PmSyncDeps {
	syncGithub(projectId: string): Promise<PmSyncResult>;
}

export type PmSyncOutcome =
	| { kind: "synced"; result: PmSyncResult }
	| { kind: "failed"; error: string }
	| { kind: "timeout" };

export interface PmSyncBanner {
	kind: "error" | "changed";
	message: string;
}

/**
 * 同步结果 → 面板提示（单一文案来源，便于单测）。
 * 失败/超时必须常驻面板（#66 教训：只弹 5 秒 toast，用户只见空看板不知原因）；
 * 有变更时提示成功；无任何变更返回 null（不打扰）。
 */
export function syncBannerFromOutcome(
	outcome: PmSyncOutcome,
): PmSyncBanner | null {
	if (outcome.kind === "failed")
		return { kind: "error", message: "GitHub 同步失败：" + outcome.error };
	if (outcome.kind === "timeout")
		return { kind: "error", message: "GitHub 同步超时，可稍后手动刷新" };
	const { created, updated, moved } = outcome.result;
	if (created + updated + moved > 0)
		return {
			kind: "changed",
			message: `GitHub 同步：新增 ${created} · 更新 ${updated} · 迁移 ${moved}`,
		};
	return null;
}

/**
 * 带超时兜底的 GitHub 同步：正常返回 synced/failed；同步悬挂（promise 永不
 * settle）时 timeoutMs 后收敛为 timeout，调用方据此 toast 并放弃本次等待。
 */
export async function pmSyncWithTimeout(
	projectId: string,
	deps: PmSyncDeps,
	timeoutMs: number = PM_SYNC_TIMEOUT_MS,
): Promise<PmSyncOutcome> {
	let timer: ReturnType<typeof setTimeout> | undefined;
	try {
		return await Promise.race([
			deps
				.syncGithub(projectId)
				.then((result): PmSyncOutcome => ({ kind: "synced", result })),
			new Promise<PmSyncOutcome>((resolve) => {
				timer = setTimeout(() => resolve({ kind: "timeout" }), timeoutMs);
			}),
		]);
	} catch (e) {
		return { kind: "failed", error: String(e) };
	} finally {
		clearTimeout(timer);
	}
}
