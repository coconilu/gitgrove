import { RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import * as api from "../../api";
import { useStore } from "../../store";
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
	EMPTY_FILTER,
	filterItems,
	loadPmLocalData,
	PRIORITIES,
	PRIORITY_LABELS,
	pmSyncWithTimeout,
	repoName,
} from "./model";

export type PmView = "board" | "milestones";

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
	const [error, setError] = useState("");
	const [filter, setFilter] = useState<BoardFilter>(EMPTY_FILTER);
	const [itemDialog, setItemDialog] = useState<{
		item: PmItem | null;
		status: string | null;
	} | null>(null);
	const [msDialog, setMsDialog] = useState<PmMilestoneWithStats | null | "new">(
		null,
	);
	const [columnsOpen, setColumnsOpen] = useState(false);
	const [refreshing, setRefreshing] = useState(false);

	/** 只加载本地数据（SQLite 毫秒级）。GitHub 同步不在此处——同步悬挂/失败
	 * 都不能把看板卡在「正在加载」（#61 的根因就是旧版在此先 await 同步） */
	const loadLocal = useCallback(async () => {
		setError("");
		try {
			const { items, milestones } = await loadPmLocalData({
				listItems: api.pmListItems,
				listMilestones: api.pmListMilestones,
			});
			setItems(items);
			setMilestones(milestones);
		} catch (e) {
			setError(String(e));
		}
		try {
			setStatuses(await api.pmListStatuses());
		} catch {
			// 后端 pm_list_statuses 未就绪时回退默认四列
		}
	}, []);

	/** GitHub 同步后台执行：有变更时静默重载看板，失败/超时只 toast；
	 * 同步悬挂由 PM_SYNC_TIMEOUT_MS 收敛放弃等待，永不阻断看板（#61） */
	const syncInBackground = useCallback(() => {
		void pmSyncWithTimeout(p.id, { syncGithub: api.pmSyncGithub }).then(
			(outcome) => {
				if (outcome.kind === "synced") {
					const { created, updated, moved } = outcome.result;
					if (created + updated + moved > 0) {
						toast(
							`GitHub 同步：新增 ${created} · 更新 ${updated} · 迁移 ${moved}`,
						);
						void loadLocal();
					}
				} else if (outcome.kind === "failed") {
					toast("GitHub 同步失败：" + outcome.error);
				} else {
					toast("GitHub 同步超时，可稍后手动刷新");
				}
			},
		);
	}, [p.id, loadLocal, toast]);

	const hasGithub = p.providerIdentity?.provider === "github";
	useEffect(() => {
		// 本地数据先行渲染；同步 fire-and-forget，完成/失败/悬挂都不阻塞
		void loadLocal();
		if (hasGithub) syncInBackground();
	}, [loadLocal, syncInBackground, hasGithub]);

	const refresh = async () => {
		setRefreshing(true);
		await loadLocal();
		if (hasGithub) syncInBackground();
		setRefreshing(false);
	};

	// ---- item 变更：本地状态即时更新，失败回退到整表重载 ----
	const createItem = async (input: PmNewItem) => {
		const created = await api.pmCreateItem(input);
		setItems((list) => [...(list ?? []), created]);
		void api
			.pmListMilestones()
			.then(setMilestones)
			.catch(() => {});
		toast("任务已创建");
	};
	const updateItem = async (item: PmItem) => {
		const updated = await api.pmUpdateItem(item);
		setItems((list) =>
			(list ?? []).map((i) => (i.id === updated.id ? updated : i)),
		);
		void api
			.pmListMilestones()
			.then(setMilestones)
			.catch(() => {});
		toast("任务已保存");
	};
	const deleteItem = async (item: PmItem) => {
		await api.pmDeleteItem(item.id);
		setItems((list) => (list ?? []).filter((i) => i.id !== item.id));
		void api
			.pmListMilestones()
			.then(setMilestones)
			.catch(() => {});
		toast("任务已删除");
	};
	/** 拖拽流转：乐观更新，服务端返回的权威 order 再回填 */
	const moveItem = async (
		itemId: string,
		toStatus: string,
		beforeItemId: string | null,
	) => {
		const prev = (items ?? []).find((i) => i.id === itemId);
		setItems((list) => {
			const current = (list ?? []).filter((i) => i.id !== itemId);
			const active = (list ?? []).find((i) => i.id === itemId);
			if (!active) return list;
			const moved = {
				...active,
				status: toStatus,
				order: beforeItemId
					? (current.find((i) => i.id === beforeItemId)?.order ?? active.order)
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
			await loadLocal();
		}
	};

	// ---- milestone 变更 ----
	const saveMilestone = async (
		editing: PmMilestoneWithStats | null,
		title: string,
		description: string,
		dueDate: string | null,
		status: string,
	) => {
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
	};
	const deleteMilestone = (m: PmMilestoneWithStats) =>
		openDialog({
			kind: "confirm",
			title: "删除里程碑 " + m.title + "？",
			message:
				"里程碑将被删除，其下 " + m.total + " 个任务的里程碑归属会被清空。",
			okText: "删除",
			danger: true,
			onSubmit: async () => {
				await api.pmDeleteMilestone(m.id);
				setMilestones((list) => (list ?? []).filter((x) => x.id !== m.id));
				setItems((list) =>
					(list ?? []).map((i) =>
						i.milestoneId === m.id ? { ...i, milestoneId: null } : i,
					),
				);
				toast("里程碑已删除");
			},
		});

	const saveStatuses = async (next: PmStatusDef[]) => {
		const saved = await api.pmUpdateStatuses(next);
		setStatuses(saved);
		await loadLocal();
		toast("看板列已更新");
	};

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

	const loading = items === null || milestones === null;
	if (loading || error)
		return (
			<ResourceState
				loading={loading && !error}
				error={error}
				onRetry={() => void loadLocal()}
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
					busy={refreshing}
					onClick={() => void refresh()}
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
