import { RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as api from "../../api";
import {
	peekSwr,
	pokeSwr,
	type SwrState,
	swrFetch,
	useStore,
} from "../../store";
import type {
	PmItem,
	PmMilestoneWithStats,
	PmNewItem,
	PmStatusDef,
	Project,
} from "../../types";
import IconButton from "../IconButton";
import { ResourceState } from "../ResourceState";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Select } from "../ui/select";
import BoardView from "./BoardView";
import ColumnsDialog from "./ColumnsDialog";
import ItemDialog from "./ItemDialog";
import MilestoneDialog from "./MilestoneDialog";
import MilestoneView from "./MilestoneView";
import {
	type BoardFilter,
	createRefreshGate,
	DEFAULT_STATUSES,
	EMPTY_FILTER,
	filterItems,
	type PmSyncBanner,
	PRIORITIES,
	PRIORITY_LABELS,
	pmSyncWithTimeout,
	repoName,
	syncBannerFromOutcome,
} from "./model";

export type PmView = "board" | "milestones";

function initialPmSwr<T>(key: string): SwrState<T> {
	const cached = peekSwr<T>(key);
	return cached !== undefined
		? { data: cached, error: "", loading: false, refreshing: true }
		: { data: null, error: "", loading: true, refreshing: false };
}

/**
 * 看板 SWR 数据源（#77）：进入页签即订阅——缓存命中先显上次快照，后台刷新完成后
 * 经 apply 落到组件状态。缓存在 store.ts 的模块级 resourceCache 里，随页签卸载仍在，
 * 切回来立即可显。revision 递增触发整表重载；返回值带 key，跨项目切换时用于
 * 识别数据归属（旧项目的镜像写入不得污染新项目的缓存键）。
 */
function usePmSwr<T>(
	key: string,
	load: () => Promise<T>,
	apply: (data: T) => void,
	revision: number,
): SwrState<T> & { key: string } {
	const [state, setState] = useState<SwrState<T> & { key: string }>(() => ({
		key,
		...initialPmSwr(key),
	}));
	const latest = useRef({ load, apply });
	latest.current = { load, apply };
	// biome-ignore lint/correctness/useExhaustiveDependencies: revision 是显式重载信号
	useEffect(() => {
		return swrFetch<T>({
			key,
			cache: true,
			load: () => latest.current.load(),
			emit: (next) => {
				setState({ key, ...next });
				// 刷新失败时 swrFetch 回传请求起点的旧快照：跳过 apply，
				// 保留本地可能已乐观推进的状态
				if (next.data !== null && !next.error) latest.current.apply(next.data);
			},
		});
	}, [key, revision]);
	return state.key === key ? state : { key, ...initialPmSwr(key) };
}

/** 自研项目管理面板：看板 + 里程碑两个视图，数据本地优先（SQLite via Tauri commands） */
export default function PmPanel({ p }: { p: Project }) {
	const toast = useStore((s) => s.toast);
	const openDialog = useStore((s) => s.openDialog);
	const [view, setView] = useState<PmView>("board");
	const [statuses, setStatuses] = useState<PmStatusDef[]>(DEFAULT_STATUSES);
	const [items, setItems] = useState<PmItem[] | null>(null);
	const [milestones, setMilestones] = useState<PmMilestoneWithStats[] | null>(
		null,
	);
	const [filter, setFilter] = useState<BoardFilter>(EMPTY_FILTER);
	const [itemDialog, setItemDialog] = useState<{
		item: PmItem | null;
		status: string | null;
	} | null>(null);
	const [msDialog, setMsDialog] = useState<PmMilestoneWithStats | null | "new">(
		null,
	);
	const [columnsOpen, setColumnsOpen] = useState(false);
	const [syncing, setSyncing] = useState(false);
	/** 同步结果横幅：失败/超时常驻展示（#66 教训：只弹 toast 等于静默吞错） */
	const [syncBanner, setSyncBanner] = useState<PmSyncBanner | null>(null);
	// SWR 整表重载代际：手动刷新 / 同步变更后 / 错误重试共用一个信号
	const [revision, setRevision] = useState(0);
	/** 拖拽乐观更新与后台刷新的竞态闸门（#77） */
	const gateRef = useRef(createRefreshGate());
	const reloadLocal = useCallback(() => setRevision((n) => n + 1), []);

	// 整表刷新结果经闸门落状态：拖拽进行中扣下，出闸后由调用方重载收口
	const applyItems = useCallback((data: PmItem[]) => {
		if (gateRef.current.offer()) setItems(data);
	}, []);
	const applyMilestones = useCallback(
		(data: PmMilestoneWithStats[]) => setMilestones(data),
		[],
	);
	const applyStatuses = useCallback(
		(data: PmStatusDef[]) => setStatuses(data),
		[],
	);

	const loadStatuses = useCallback(async (): Promise<PmStatusDef[]> => {
		try {
			return await api.pmListStatuses();
		} catch {
			// 后端 pm_list_statuses 未就绪时回退默认四列
			return DEFAULT_STATUSES;
		}
	}, []);

	// 三类本地数据各自接 swrFetch，键按 projectId 隔离（与 ProjectDetail 的 p.id + ":xxx" 同约定）
	const itemsStatus = usePmSwr(
		p.id + ":pm:items",
		() => api.pmListItems(null),
		applyItems,
		revision,
	);
	const milestonesStatus = usePmSwr(
		p.id + ":pm:milestones",
		() => api.pmListMilestones(),
		applyMilestones,
		revision,
	);
	const statusesStatus = usePmSwr(
		p.id + ":pm:statuses",
		loadStatuses,
		applyStatuses,
		revision,
	);

	// 本地乐观变更镜像进模块级缓存：切走页签再回来立即看到最新内容（含未同步变更）。
	// key 归属校验防止切项目瞬间的旧数据写进新项目的缓存键
	useEffect(() => {
		if (itemsStatus.key === p.id + ":pm:items" && items)
			pokeSwr(p.id + ":pm:items", items);
	}, [itemsStatus.key, items, p.id]);
	useEffect(() => {
		if (milestonesStatus.key === p.id + ":pm:milestones" && milestones)
			pokeSwr(p.id + ":pm:milestones", milestones);
	}, [milestonesStatus.key, milestones, p.id]);
	useEffect(() => {
		// loading 中还只有回退默认列，不写入缓存
		if (statusesStatus.key === p.id + ":pm:statuses" && !statusesStatus.loading)
			pokeSwr(p.id + ":pm:statuses", statuses);
	}, [statusesStatus.key, statusesStatus.loading, statuses, p.id]);

	// 切项目重置闸门：上一个项目残留的挂起计数不能吞掉新项目的刷新
	// biome-ignore lint/correctness/useExhaustiveDependencies: p.id 变化本身即重置信号
	useEffect(() => {
		gateRef.current = createRefreshGate();
	}, [p.id]);

	/** GitHub 同步后台执行：失败/超时以常驻横幅展示并可重试（只弹 toast 用户
	 * 会错过，#66 的看板全空就是这么来的）；成功有变更时 toast + 静默重载；
	 * 同步悬挂由 PM_SYNC_TIMEOUT_MS 收敛放弃等待，永不阻断看板（#61） */
	const syncInBackground = useCallback(() => {
		setSyncing(true);
		void pmSyncWithTimeout(p.id, { syncGithub: api.pmSyncGithub }).then(
			(outcome) => {
				setSyncing(false);
				const banner = syncBannerFromOutcome(outcome);
				setSyncBanner(banner?.kind === "error" ? banner : null);
				if (banner?.kind === "changed") {
					toast(banner.message);
					reloadLocal(); // 经闸门：拖拽进行中先扣下，出闸后重载
				}
			},
		);
	}, [p.id, reloadLocal, toast]);

	const hasGithub = p.providerIdentity?.provider === "github";
	useEffect(() => {
		// 本地数据由 SWR 先行渲染（缓存命中立显、无缓存走 SQLite 毫秒级）；
		// 同步 fire-and-forget，完成/失败/悬挂都不阻塞
		if (hasGithub) syncInBackground();
	}, [syncInBackground, hasGithub]);

	const refresh = () => {
		reloadLocal();
		if (hasGithub) syncInBackground();
	};

	/** 变更入闸执行：期间到达的整表刷新一律扣下，结束后有被扣下的就重载收口 */
	const runMutate = async (fn: () => Promise<void>) => {
		const gate = gateRef.current;
		gate.begin();
		try {
			await fn();
		} finally {
			if (gate.end()) reloadLocal();
		}
	};

	// ---- item 变更：本地状态即时更新，失败回退到整表重载；期间整表刷新被闸门扣下 ----
	const createItem = (input: PmNewItem) =>
		runMutate(async () => {
			const created = await api.pmCreateItem(input);
			setItems((list) => [...(list ?? []), created]);
			void api
				.pmListMilestones()
				.then(setMilestones)
				.catch(() => {});
			toast("任务已创建");
		});
	const updateItem = (item: PmItem) =>
		runMutate(async () => {
			const updated = await api.pmUpdateItem(item);
			setItems((list) =>
				(list ?? []).map((i) => (i.id === updated.id ? updated : i)),
			);
			void api
				.pmListMilestones()
				.then(setMilestones)
				.catch(() => {});
			toast("任务已保存");
		});
	const deleteItem = (item: PmItem) =>
		runMutate(async () => {
			await api.pmDeleteItem(item.id);
			setItems((list) => (list ?? []).filter((i) => i.id !== item.id));
			void api
				.pmListMilestones()
				.then(setMilestones)
				.catch(() => {});
			toast("任务已删除");
		});
	/** 拖拽流转：乐观更新，服务端返回的权威 order 再回填；入闸保证期间的
	 * 整表刷新不覆盖拖拽中的乐观顺序（#77） */
	const moveItem = (
		itemId: string,
		toStatus: string,
		beforeItemId: string | null,
	) =>
		runMutate(async () => {
			const prev = (items ?? []).find((i) => i.id === itemId);
			setItems((list) => {
				const current = (list ?? []).filter((i) => i.id !== itemId);
				const active = (list ?? []).find((i) => i.id === itemId);
				if (!active) return list;
				const moved = {
					...active,
					status: toStatus,
					order: beforeItemId
						? (current.find((i) => i.id === beforeItemId)?.order ??
							active.order)
						: "\uffff",
				};
				const at = beforeItemId
					? current.findIndex((i) => i.id === beforeItemId)
					: current.length;
				current.splice(at < 0 ? current.length : at, 0, moved);
				return [...current];
			});
			try {
				let updated = await api.pmMoveItem(itemId, toStatus, beforeItemId);
				// 拖动 GitHub 卡片换列 → 置 manualLock（人工接管列位置，同步不再自动迁移）；
				// PUT 全字段语义：回传 move 返回的完整 item，order/createdAt/githubRef 服务端保留
				if (prev?.githubRef && !prev.manualLock && prev.status !== toStatus) {
					updated = await api.pmUpdateItem({ ...updated, manualLock: true });
				}
				setItems((list) =>
					(list ?? []).map((i) => (i.id === updated.id ? updated : i)),
				);
				void api
					.pmListMilestones()
					.then(setMilestones)
					.catch(() => {});
			} catch (e) {
				toast("移动失败：" + String(e));
				reloadLocal(); // 失败回滚：以数据库为准；结果被扣下时由出闸重载兜底
			}
		});

	// ---- milestone 变更 ----
	const saveMilestone = (
		editing: PmMilestoneWithStats | null,
		title: string,
		description: string,
		dueDate: string | null,
		status: string,
	) =>
		runMutate(async () => {
			if (editing) {
				const updated = await api.pmUpdateMilestone({
					...editing,
					title,
					description,
					dueDate,
					status,
				});
				setMilestones((list) =>
					(list ?? []).map((m) =>
						m.id === updated.id ? { ...m, ...updated } : m,
					),
				);
				toast("里程碑已保存");
			} else {
				await api.pmCreateMilestone(title, description || null, dueDate);
				setMilestones(await api.pmListMilestones());
				toast("里程碑已创建");
			}
		});
	const deleteMilestone = (m: PmMilestoneWithStats) =>
		openDialog({
			kind: "confirm",
			title: "删除里程碑 " + m.title + "？",
			message:
				"里程碑将被删除，其下 " + m.total + " 个任务的里程碑归属会被清空。",
			okText: "删除",
			danger: true,
			onSubmit: () =>
				runMutate(async () => {
					await api.pmDeleteMilestone(m.id);
					setMilestones((list) => (list ?? []).filter((x) => x.id !== m.id));
					setItems((list) =>
						(list ?? []).map((i) =>
							i.milestoneId === m.id ? { ...i, milestoneId: null } : i,
						),
					);
					toast("里程碑已删除");
				}),
		});

	const saveStatuses = (next: PmStatusDef[]) =>
		runMutate(async () => {
			const saved = await api.pmUpdateStatuses(next);
			setStatuses(saved);
			reloadLocal(); // 列定义变化影响任务归属，整表重载看板数据
			toast("看板列已更新");
		});

	const filtered = useMemo(
		() => filterItems(items ?? [], filter),
		[items, filter],
	);
	const repoOptions = useMemo(() => {
		const paths = new Set<string>();
		for (const i of items ?? []) if (i.repoPath) paths.add(i.repoPath);
		if (p.localPath) paths.add(p.localPath);
		return [...paths].sort();
	}, [items, p.localPath]);
	const labelOptions = useMemo(
		() => [...new Set((items ?? []).flatMap((i) => i.labels))].sort(),
		[items],
	);

	// 首屏加载（无缓存可显）或任一本地资源刷新失败 → 整屏 ResourceState
	const loadError = itemsStatus.error || milestonesStatus.error;
	const loading = items === null || milestones === null;
	if (loading || loadError)
		return (
			<ResourceState
				loading={loading && !loadError}
				error={loadError}
				onRetry={reloadLocal}
				title="加载项目数据"
			/>
		);

	const setF = (patch: Partial<BoardFilter>) =>
		setFilter((f) => ({ ...f, ...patch }));
	const NONE = "__none__";
	const selectValue = (v: string | null) => v ?? NONE;
	const fromSelect = (v: string) => (v === NONE ? null : v);

	return (
		<div className="pm-panel">
			<div className="pm-toolbar">
				<span className="pm-title">
					项目 · {view === "board" ? "看板" : "里程碑"}
				</span>
				<div className="pm-view-switch" role="tablist" aria-label="视图切换">
					<button
						className={"pm-chip" + (view === "board" ? " on" : "")}
						aria-pressed={view === "board"}
						onClick={() => setView("board")}
					>
						看板
					</button>
					<button
						className={"pm-chip" + (view === "milestones" ? " on" : "")}
						aria-pressed={view === "milestones"}
						onClick={() => setView("milestones")}
					>
						里程碑
					</button>
				</div>
				{view === "board" && (
					<>
						<Select
							aria-label="按里程碑过滤"
							className="pm-filter"
							value={selectValue(filter.milestoneId)}
							onValueChange={(v) => setF({ milestoneId: fromSelect(v) })}
							options={[
								{ value: NONE, label: "里程碑: 全部" },
								...(milestones ?? []).map((m) => ({
									value: m.id,
									label: "🏁 " + m.title,
								})),
							]}
						/>
						<Select
							aria-label="按仓库过滤"
							className="pm-filter"
							value={selectValue(filter.repoPath)}
							onValueChange={(v) => setF({ repoPath: fromSelect(v) })}
							options={[
								{ value: NONE, label: "仓库: 全部" },
								...repoOptions.map((r) => ({
									value: r,
									label: repoName(r),
								})),
							]}
						/>
						<Select
							aria-label="按标签过滤"
							className="pm-filter"
							value={selectValue(filter.label)}
							onValueChange={(v) => setF({ label: fromSelect(v) })}
							options={[
								{ value: NONE, label: "标签: 全部" },
								...labelOptions.map((l) => ({ value: l, label: l })),
							]}
						/>
						<Select
							aria-label="按优先级过滤"
							className="pm-filter"
							value={selectValue(filter.priority)}
							onValueChange={(v) => setF({ priority: fromSelect(v) })}
							options={[
								{ value: NONE, label: "优先级: 全部" },
								...PRIORITIES.map((pr) => ({
									value: pr,
									label: PRIORITY_LABELS[pr],
								})),
							]}
						/>
					</>
				)}
				<Input
					className="pm-search"
					aria-label="搜索任务"
					placeholder="搜索任务…"
					value={filter.search}
					onChange={(e) => setF({ search: e.target.value })}
				/>
				<IconButton
					label="刷新项目数据"
					icon={RefreshCw}
					// 本地加载/后台刷新或 GitHub 同步进行中 → 图标旋转（CSS 侧
					// .icon-button[aria-busy] 动画，尊重 prefers-reduced-motion）
					busy={
						syncing ||
						itemsStatus.loading ||
						itemsStatus.refreshing ||
						milestonesStatus.loading ||
						milestonesStatus.refreshing ||
						statusesStatus.loading ||
						statusesStatus.refreshing
					}
					onClick={refresh}
				/>
				{view === "board" && (
					<>
						<Button size="sm" onClick={() => setColumnsOpen(true)}>
							列管理
						</Button>
						<Button
							variant="primary"
							size="sm"
							onClick={() => setItemDialog({ item: null, status: null })}
						>
							+ 新建任务
						</Button>
					</>
				)}
				{view === "milestones" && (
					<Button
						variant="primary"
						size="sm"
						onClick={() => setMsDialog("new")}
					>
						+ 新建里程碑
					</Button>
				)}
			</div>
			{syncBanner?.kind === "error" && (
				<div className="inline-error" role="alert">
					<span>{syncBanner.message}</span>
					<button
						className="btn sm"
						onClick={() => {
							setSyncBanner(null);
							syncInBackground();
						}}
					>
						重试同步
					</button>
				</div>
			)}
			{view === "board" ? (
				<BoardView
					items={filtered}
					statuses={statuses}
					milestones={milestones ?? []}
					onMove={moveItem}
					onOpen={(item) => setItemDialog({ item, status: null })}
					onAdd={(status) => setItemDialog({ item: null, status })}
				/>
			) : (
				<MilestoneView
					items={filtered}
					allItems={items ?? []}
					milestones={milestones ?? []}
					statuses={statuses}
					onEdit={(m) => setMsDialog(m)}
					onDelete={deleteMilestone}
					onEnterBoard={() => setView("board")}
				/>
			)}
			{itemDialog && (
				<ItemDialog
					item={itemDialog.item}
					presetStatus={itemDialog.status}
					statuses={statuses}
					milestones={milestones ?? []}
					repoOptions={repoOptions}
					defaultRepoPath={p.localPath}
					onClose={() => setItemDialog(null)}
					onCreate={createItem}
					onUpdate={updateItem}
					onDelete={deleteItem}
				/>
			)}
			{msDialog !== null && (
				<MilestoneDialog
					editing={msDialog === "new" ? null : msDialog}
					onClose={() => setMsDialog(null)}
					onSave={saveMilestone}
				/>
			)}
			{columnsOpen && (
				<ColumnsDialog
					statuses={statuses}
					items={items ?? []}
					onClose={() => setColumnsOpen(false)}
					onSave={saveStatuses}
				/>
			)}
		</div>
	);
}
