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

// ---- done 列折叠（#80）：done 只增不减，上百张卡片会卡死渲染 ----

/** 折叠批次：默认显示最近 20 张，「显示更早」每批再展开 20 张 */
export const DONE_COLLAPSE_BATCH = 20;

/** 后端 items.closedAt（进入最后一列的时间，unix 秒，v4 迁移）；老数据为 null */

/** done 排序键：closedAt 优先（标题/标签刷新会扰动 updatedAt），老数据回退 updatedAt */
export function doneRecencyKey(item: PmItem): number {
	return item.closedAt ?? item.updatedAt;
}

/** done 列按最近完成倒序 */
export function doneRecencySort(items: PmItem[]): PmItem[] {
	return [...items].sort((a, b) => doneRecencyKey(b) - doneRecencyKey(a));
}

/** 折叠是否失效：任一筛选器非「全部」或搜索框有输入 → 渲染全部命中卡片 */
export function filterActive(f: BoardFilter): boolean {
	return (
		f.milestoneId !== null ||
		f.repoPath !== null ||
		f.label !== null ||
		f.priority !== null ||
		f.search.trim() !== ""
	);
}

export interface DoneColumnView {
	visible: PmItem[];
	hiddenCount: number;
}

/**
 * done 列折叠视图：始终按「最近完成」倒序（closedAt ?? updatedAt）——
 * 折叠态与展开态同一顺序，展开只是连续追加更早的卡片、不重排。
 * 折叠只在无筛选/搜索时生效：有筛选时渲染全部命中卡片，排序不变。
 * done 是终态列，列内按完成时间排列；列内手动拖拽不再改变显示顺序。
 */
export function doneColumnView(
	items: PmItem[],
	filter: BoardFilter,
	limit: number,
): DoneColumnView {
	const sorted = doneRecencySort(items);
	if (filterActive(filter) || sorted.length <= limit)
		return { visible: sorted, hiddenCount: 0 };
	return {
		visible: sorted.slice(0, limit),
		hiddenCount: sorted.length - limit,
	};
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

// ---- 拖拽回写（#83）：跨进/跨出最后一列的 GitHub 卡片改 issue state ----

export interface WritebackPlan {
	/** true = 关闭 issue（拖入 done），false = 重开（拖出 done） */
	closed: boolean;
}

/**
 * 这次拖拽是否需要回写 GitHub：只有「有合法 githubRef 且跨列进/出最后一列」才回写。
 * 列内排序、非 done 列之间的移动、本地卡片（无 githubRef）、老数据里的非法 ref
 * 一律返回 null。manualLock 不参与判断——锁定只约束同步引擎的自动迁移，
 * 手动拖拽始终生效。
 */
export function writebackPlan(
	item: PmItem,
	toStatus: string,
	doneId: string,
): WritebackPlan | null {
	if (!item.githubRef || !parseGithubRef(item.githubRef)) return null;
	if (item.status === toStatus) return null;
	if (toStatus === doneId) return { closed: true };
	if (item.status === doneId) return { closed: false };
	return null;
}

export interface PmMoveDeps {
	setIssueState(itemId: string, closed: boolean): Promise<void>;
	moveItem(
		itemId: string,
		toStatus: string,
		beforeItemId: string | null,
	): Promise<PmItem>;
	updateItem(item: PmItem): Promise<PmItem>;
}

export type PmMoveOutcome =
	| { ok: true; item: PmItem }
	| { ok: false; stage: "writeback" | "move"; error: string };

/**
 * 拖拽落库编排：先回写 GitHub（拖入 done → close、拖出 → reopen），成功后再
 * pm_move_item —— closedAt 的打点/清空因此与本地拖动、同步迁移共用一条路径。
 * 回写失败不做任何本地移动（调用方回滚乐观列 + toast GitHub 原始错误）；
 * 拖动 GitHub 卡片跨列照旧置 manualLock。
 *
 * closedAt 的精度（已知差异，不做补偿）：打点值是本地移动那一刻的 now，不是
 * PATCH 响应里的 GitHub 权威 closed_at——两者相差一个往返（秒级）。`pm_sync_github`
 * 的 backfill 只在 `closed_at IS NULL` 时生效（sync.rs），本地已打点故永不覆盖。
 * 该值只喂 done 列折叠排序（doneRecencyKey），秒级差异不影响分批与倒序。
 */
export async function performMove(
	deps: PmMoveDeps,
	prev: PmItem,
	toStatus: string,
	beforeItemId: string | null,
	doneId: string,
): Promise<PmMoveOutcome> {
	const plan = writebackPlan(prev, toStatus, doneId);
	if (plan) {
		try {
			await deps.setIssueState(prev.id, plan.closed);
		} catch (e) {
			return { ok: false, stage: "writeback", error: String(e) };
		}
	}
	let item: PmItem;
	try {
		item = await deps.moveItem(prev.id, toStatus, beforeItemId);
	} catch (e) {
		// 回写已成功时只可能「远端已改、本地没动」：PATCH 的往返窗口里落点前一张
		// 卡可能被并发迁走（后台 sync 自动迁移 / 用户又拖了一次）→ 报 beforeItem
		// 不在目标列。此时用列尾重试一次保住用户意图；PATCH 幂等且此处不重发回写。
		// 不需要回写的本地拖拽不重试，保持既有失败语义（todo/doing 行为不变）。
		if (!plan || beforeItemId === null)
			return { ok: false, stage: "move", error: String(e) };
		try {
			item = await deps.moveItem(prev.id, toStatus, null);
		} catch (e2) {
			return { ok: false, stage: "move", error: String(e2) };
		}
	}
	try {
		// 拖动 GitHub 卡片换列 → 置 manualLock（人工接管列位置，同步不再自动迁移）；
		// PUT 全字段语义：回传 move 返回的完整 item，order/createdAt/githubRef 服务端保留
		if (prev.githubRef && !prev.manualLock && prev.status !== toStatus)
			item = await deps.updateItem({ ...item, manualLock: true });
	} catch (e) {
		// 移动已落库、只是置锁失败：文案归到 move 阶段，卡片位置由 reloadLocal 收口
		return { ok: false, stage: "move", error: String(e) };
	}
	return { ok: true, item };
}

// ---- 数据加载编排（#61）：本地数据先渲染，GitHub 同步后台化 ----

/** 前端同步超时兜底：后端网络有界（connect 5s / 请求 10s），此处再兜住 gh CLI、
 * keyring 等无法覆盖的悬挂——超时后放弃等待当次同步，看板不受影响 */
export const PM_SYNC_TIMEOUT_MS = 15_000;

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
