import {
	closestCorners,
	DndContext,
	type DragEndEvent,
	DragOverlay,
	type DragStartEvent,
	KeyboardSensor,
	PointerSensor,
	useDroppable,
	useSensor,
	useSensors,
} from "@dnd-kit/core";
import {
	SortableContext,
	sortableKeyboardCoordinates,
	useSortable,
	verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Lock } from "lucide-react";
import { useState } from "react";
import { useStore } from "../../store";
import type { PmItem, PmMilestoneWithStats, PmStatusDef } from "../../types";
import {
	computeMove,
	dueInfo,
	groupByStatus,
	issueUrl,
	parseGithubRef,
	repoName,
	todayString,
} from "./model";

interface BoardProps {
	items: PmItem[];
	statuses: PmStatusDef[];
	milestones: PmMilestoneWithStats[];
	onMove: (
		itemId: string,
		toStatus: string,
		beforeItemId: string | null,
	) => Promise<void>;
	onOpen: (item: PmItem) => void;
	onAdd: (status: string) => void;
}

const COLUMN_DOTS = [
	"var(--faint)",
	"var(--yellow)",
	"var(--accent)",
	"var(--green)",
];

export default function BoardView({
	items,
	statuses,
	milestones,
	onMove,
	onOpen,
	onAdd,
}: BoardProps) {
	const [activeId, setActiveId] = useState<string | null>(null);
	const sensors = useSensors(
		// 4px 位移才触发拖拽，保留点击打开编辑对话框
		useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
		useSensor(KeyboardSensor, {
			coordinateGetter: sortableKeyboardCoordinates,
		}),
	);
	const groups = groupByStatus(items, statuses);
	const active = activeId ? items.find((i) => i.id === activeId) : null;

	const onDragStart = (e: DragStartEvent) => setActiveId(String(e.active.id));
	const onDragEnd = (e: DragEndEvent) => {
		setActiveId(null);
		const plan = computeMove(
			items,
			statuses,
			String(e.active.id),
			e.over ? String(e.over.id) : null,
		);
		if (plan)
			void onMove(String(e.active.id), plan.toStatus, plan.beforeItemId);
	};

	return (
		<DndContext
			sensors={sensors}
			collisionDetection={closestCorners}
			onDragStart={onDragStart}
			onDragEnd={onDragEnd}
			onDragCancel={() => setActiveId(null)}
		>
			<div className="pm-board">
				{statuses.map((status, idx) => (
					<Column
						key={status.id}
						status={status}
						dot={COLUMN_DOTS[idx % COLUMN_DOTS.length]}
						items={groups.get(status.id) ?? []}
						milestones={milestones}
						onOpen={onOpen}
						onAdd={onAdd}
					/>
				))}
			</div>
			<DragOverlay>
				{active ? (
					<CardFace item={active} milestones={milestones} dragging />
				) : null}
			</DragOverlay>
		</DndContext>
	);
}

function Column({
	status,
	dot,
	items,
	milestones,
	onOpen,
	onAdd,
}: {
	status: PmStatusDef;
	dot: string;
	items: PmItem[];
	milestones: PmMilestoneWithStats[];
	onOpen: (item: PmItem) => void;
	onAdd: (status: string) => void;
}) {
	const { setNodeRef, isOver } = useDroppable({ id: status.id });
	return (
		<section
			className={"pm-col" + (isOver ? " over" : "")}
			aria-label={status.name}
		>
			<header className="pm-col-head">
				<span className="pm-dot" style={{ background: dot }} />
				{status.name}
				<span className="pm-count">{items.length}</span>
			</header>
			<SortableContext
				items={items.map((i) => i.id)}
				strategy={verticalListSortingStrategy}
			>
				<div className="pm-col-body" ref={setNodeRef}>
					{items.map((item) => (
						<SortableCard
							key={item.id}
							item={item}
							milestones={milestones}
							onOpen={onOpen}
						/>
					))}
					<button className="pm-add-card" onClick={() => onAdd(status.id)}>
						+ 添加
					</button>
				</div>
			</SortableContext>
		</section>
	);
}

function SortableCard({
	item,
	milestones,
	onOpen,
}: {
	item: PmItem;
	milestones: PmMilestoneWithStats[];
	onOpen: (item: PmItem) => void;
}) {
	const {
		attributes,
		listeners,
		setNodeRef,
		transform,
		transition,
		isDragging,
	} = useSortable({ id: item.id });
	return (
		<div
			ref={setNodeRef}
			style={{
				transform: transform
					? `translate3d(${transform.x}px, ${transform.y}px, 0)`
					: undefined,
				transition,
				opacity: isDragging ? 0.4 : undefined,
			}}
			{...attributes}
			{...listeners}
		>
			<CardFace
				item={item}
				milestones={milestones}
				onOpen={() => onOpen(item)}
			/>
		</div>
	);
}

/** 卡片内容：标题 + GitHub 引用徽标 / 锁定标记 / 优先级点 / 里程碑 / 仓库 / 截止日 / 标签 */
function CardFace({
	item,
	milestones,
	dragging,
	onOpen,
}: {
	item: PmItem;
	milestones: PmMilestoneWithStats[];
	dragging?: boolean;
	onOpen?: () => void;
}) {
	const toast = useStore((s) => s.toast);
	const ms = milestones.find((m) => m.id === item.milestoneId);
	const due = item.dueDate ? dueInfo(item.dueDate, todayString()) : null;
	const gref = item.githubRef ? parseGithubRef(item.githubRef) : null;
	const openIssue = () => {
		const url = item.githubRef ? issueUrl(item.githubRef) : null;
		if (url) openUrl(url).catch((e) => toast(String(e)));
	};
	return (
		<article
			className={"pm-card" + (dragging ? " dragging" : "")}
			onClick={onOpen}
			onKeyDown={
				onOpen
					? (e) => {
							if (e.key === "Enter" && e.target === e.currentTarget) onOpen();
						}
					: undefined
			}
			tabIndex={onOpen ? 0 : undefined}
			aria-label={item.title}
		>
			<div className="pm-card-title">{item.title}</div>
			<div className="pm-card-meta">
				{gref && (
					<button
						className="pm-gh-ref"
						title={`${item.githubRef} · 在 GitHub 打开`}
						onPointerDown={(e) => e.stopPropagation()}
						onClick={(e) => {
							e.stopPropagation();
							openIssue();
						}}
					>
						#{gref.number}
					</button>
				)}
				{item.priority !== "none" && (
					<span
						className={"pm-prio p-" + item.priority}
						title={"优先级：" + item.priority}
					/>
				)}
				{item.manualLock && (
					<span
						className="pm-lock"
						title="已锁定：手动拖动过，自动迁移暂停（编辑任务可解除）"
					>
						<Lock size={11} aria-hidden />
					</span>
				)}
				{ms && <span className="pm-tag ms">🏁 {ms.title}</span>}
				{item.repoPath && (
					<span className="pm-tag repo">{repoName(item.repoPath)}</span>
				)}
				{due && (
					<span className={"pm-tag due" + (due.overdue ? " over" : "")}>
						📅 {due.dueToday ? "今天" : due.short}
					</span>
				)}
				{item.labels.map((l) => (
					<span className="pm-tag" key={l}>
						{l}
					</span>
				))}
			</div>
		</article>
	);
}
