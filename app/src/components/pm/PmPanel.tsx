import { RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as api from "../../api";
import {
	invalidateSwr,
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
	DEFAULT_STATUSES,
	doneStatusId,
	EMPTY_FILTER,
	filterItems,
	type PmSyncBanner,
	PRIORITIES,
	PRIORITY_LABELS,
	performMove,
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
 * 切回来立即可显。revision 递增触发整表重载；key 在组件生命周期内不变
 * （PmPanel 随项目/页签重挂载）。
 */
function usePmSwr<T>(
	key: string,
	load: () => Promise<T>,
	apply: (data: T) => void,
	revision: number,
): SwrState<T> {
	const [state, setState] = useState<SwrState<T>>(() => initialPmSwr(key));
	const latest = useRef({ load, apply });
	latest.current = { load, apply };
	// biome-ignore lint/correctness/useExhaustiveDependencies: revision 是显式重载信号
	useEffect(() => {
		return swrFetch<T>({
			key,
			cache: true,
			load: () => latest.current.load(),
			emit: (next) => {
				setState(next);
				// 刷新失败时 swrFetch 回传请求起点的旧快照：跳过 apply，
				// 保留本地可能已乐观推进的状态
				if (next.data !== null && !next.error) latest.current.apply(next.data);
			},
		});
	}, [key, revision]);
	return state;
}

/** 自研项目管理面板：看板 + 里程碑两个视图，数据本地优先（SQLite via Tauri commands） */
export default function PmPanel({ p }: { p: Project }) {
	const toast = useStore((s) => s.toast);
	const openDialog = useStore((s) => s.openDialog);
	const [view, setView] = useState<PmView>("board");
	// 三类本地数据的缓存键，按 projectId 隔离（与 ProjectDetail 的 p.id + ":xxx" 同约定）
	const itemsKey = p.id + ":pm:items";
	const milestonesKey = p.id + ":pm:milestones";
	const statusesKey = p.id + ":pm:statuses";
	// 初始 state 直接从模块级缓存播种：缓存命中时首个 commit 就是数据，
	// 不再先画一帧整屏 loading（PmPanel 随项目/页签重挂载，播种即最新快照）
	const [items, setItems] = useState<PmItem[] | null>(
		() => peekSwr<PmItem[]>(itemsKey) ?? null,
	);
	const [milestones, setMilestones] = useState<PmMilestoneWithStats[] | null>(
		() => peekSwr<PmMilestoneWithStats[]>(milestonesKey) ?? null,
	);
	const [statuses, setStatuses] = useState<PmStatusDef[]>(
		() => peekSwr<PmStatusDef[]>(statusesKey) ?? DEFAULT_STATUSES,
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
	/** 本地乐观变更在途深度：>0 时到达的整表刷新一律不落状态。
	 * 变更开始时已 invalidateSwr 作废在途请求，这里兜底拦住作废之后才发出的读 */
	const mutatingRef = useRef(0);
	const reloadLocal = useCallback(() => setRevision((n) => n + 1), []);

	// 整表刷新结果落状态：变更在途时不应用，变更结束后由 runMutate 的
	// reloadLocal 以数据库为准收口
	const applyItems = useCallback((data: PmItem[]) => {
		if (mutatingRef.current === 0) setItems(data);
	}, []);
	const applyMilestones = useCallback((data: PmMilestoneWithStats[]) => {
		if (mutatingRef.current === 0) setMilestones(data);
	}, []);
	const applyStatuses = useCallback((data: PmStatusDef[]) => {
		if (mutatingRef.current === 0) setStatuses(data);
	}, []);

	const loadStatuses = useCallback(async (): Promise<PmStatusDef[]> => {
		try {
			return await api.pmListStatuses();
		} catch {
			// 后端 pm_list_statuses 未就绪时回退默认四列
			return DEFAULT_STATUSES;
		}
	}, []);

	// 三类本地数据各自接 swrFetch，键按 projectId 隔离
	const itemsStatus = usePmSwr(
		itemsKey,
		() => api.pmListItems(null),
		applyItems,
		revision,
	);
	const milestonesStatus = usePmSwr(
		milestonesKey,
		() => api.pmListMilestones(),
		applyMilestones,
		revision,
	);
	const statusesStatus = usePmSwr(
		statusesKey,
		loadStatuses,
		applyStatuses,
		revision,
	);

	// 本地乐观变更镜像进模块级缓存：切走页签再回来立即看到最新内容（含未同步变更）
	useEffect(() => {
		if (items) pokeSwr(itemsKey, items);
	}, [itemsKey, items]);
	useEffect(() => {
		if (milestones) pokeSwr(milestonesKey, milestones);
	}, [milestonesKey, milestones]);
	useEffect(() => {
		// 首次加载完成前只有回退默认列，不写入缓存
		if (!statusesStatus.loading) pokeSwr(statusesKey, statuses);
	}, [statusesKey, statusesStatus.loading, statuses]);

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
					// 有变更在途时该读会被暂扣，由变更结束后的 reloadLocal 收口
					reloadLocal();
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

	/** 变更执行包装：开始即 invalidateSwr 作废受影响 keys 在途的整表读（按
	 * 「请求发出时刻」判定，变更前的旧响应不再落地覆盖乐观顺序，也不污染缓存），
	 * 期间新到的读由 mutatingRef 暂扣；结束无条件 reloadLocal 以数据库为准收口 */
	const runMutate = async (fn: () => Promise<void>, keys: string[]) => {
		mutatingRef.current++;
		for (const key of keys) invalidateSwr(key);
		try {
			await fn();
		} finally {
			mutatingRef.current = Math.max(0, mutatingRef.current - 1);
			reloadLocal();
		}
	};

	// ---- item 变更：本地状态即时更新，失败回退到整表重载 ----
	const createItem = (input: PmNewItem) =>
		runMutate(async () => {
			const created = await api.pmCreateItem(input);
			setItems((list) => [...(list ?? []), created]);
			void api
				.pmListMilestones()
				.then(setMilestones)
				.catch(() => {});
			toast("任务已创建");
		}, [itemsKey, milestonesKey]);
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
		}, [itemsKey, milestonesKey]);
	const deleteItem = (item: PmItem) =>
		runMutate(async () => {
			await api.pmDeleteItem(item.id);
			setItems((list) => (list ?? []).filter((i) => i.id !== item.id));
			void api
				.pmListMilestones()
				.then(setMilestones)
				.catch(() => {});
			toast("任务已删除");
		}, [itemsKey, milestonesKey]);
	/** 拖拽流转：乐观更新，服务端返回的权威 order 再回填；作废 + 在途暂扣保证
	 * 期间的整表刷新不覆盖拖拽中的乐观顺序（#77）。
	 * GitHub 卡片跨进/跨出 done 列时先回写 issue（close/reopen，见 performMove）：
	 * 回写失败退回原列并 toast GitHub 原始错误；manualLock 只挡同步引擎的自动
	 * 迁移，手动拖拽照常生效 */
	const moveItem = (
		itemId: string,
		toStatus: string,
		beforeItemId: string | null,
	) =>
		runMutate(async () => {
			const prev = (items ?? []).find((i) => i.id === itemId);
			if (!prev) return;
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
			const outcome = await performMove(
				{
					setIssueState: api.pmSetGithubIssueState,
					moveItem: api.pmMoveItem,
					updateItem: api.pmUpdateItem,
				},
				prev,
				toStatus,
				beforeItemId,
				doneStatusId(statuses),
			);
			if (!outcome.ok) {
				// 回滚由 runMutate 结束的 reloadLocal 以数据库为准收口（本地未落库）
				toast(
					(outcome.stage === "writeback"
						? "回写 GitHub 失败："
						: "移动失败：") + outcome.error,
				);
				return;
			}
			const updated = outcome.item;
			setItems((list) =>
				(list ?? []).map((i) => (i.id === updated.id ? updated : i)),
			);
			void api
				.pmListMilestones()
				.then(setMilestones)
				.catch(() => {});
		}, [itemsKey, milestonesKey]);

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
		}, [milestonesKey]);
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
				}, [milestonesKey, itemsKey]),
		});

	const saveStatuses = (next: PmStatusDef[]) =>
		runMutate(async () => {
			const saved = await api.pmUpdateStatuses(next);
			setStatuses(saved);
			// 列定义变化影响任务归属（删列会把 item 归到第一列，同时改写 items 表），
			// 结束后 reloadLocal 整表收口
			toast("看板列已更新");
		}, [statusesKey, itemsKey]);

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

	// 整屏 ResourceState 只在「无可显数据」时使用：首屏加载中，或加载/刷新失败且
	// 没有缓存可显；有数据时刷新失败走 inline 横幅（对齐 ProjectDetail 的 SWR 约定）
	const loadError = itemsStatus.error || milestonesStatus.error;
	const loading = items === null || milestones === null;
	if (loading)
		return (
			<ResourceState
				loading={!loadError}
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
			{loadError && (
				<div className="inline-error" role="alert">
					刷新失败，仍显示上次加载的结果。
					<button className="btn sm" onClick={reloadLocal}>
						重试
					</button>
				</div>
			)}
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
					filter={filter}
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
