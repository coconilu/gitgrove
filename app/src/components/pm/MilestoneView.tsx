import type { PmItem, PmMilestoneWithStats, PmStatusDef } from "../../types";
import { ResourceState } from "../ResourceState";
import {
	DropdownMenu,
	DropdownMenuContent,
	DropdownMenuItem,
	DropdownMenuTrigger,
} from "../ui/dropdown-menu";
import {
	doneStatusId,
	dueInfo,
	focusSummary,
	milestoneProgress,
	sortMilestones,
	todayString,
} from "./model";

interface MilestoneViewProps {
	/** 当前过滤条件下的 items（行内统计用） */
	items: PmItem[];
	/** 未过滤的全量 items（今日聚焦跨里程碑聚合用） */
	allItems: PmItem[];
	milestones: PmMilestoneWithStats[];
	statuses: PmStatusDef[];
	onEdit: (m: PmMilestoneWithStats) => void;
	onDelete: (m: PmMilestoneWithStats) => void;
	onEnterBoard: () => void;
}

/** 里程碑列表：进度分段条 + 截止倒计时 + 逾期高亮 + 今日聚焦横条 */
export default function MilestoneView({
	items,
	allItems,
	milestones,
	statuses,
	onEdit,
	onDelete,
	onEnterBoard,
}: MilestoneViewProps) {
	const today = todayString();
	const doneId = doneStatusId(statuses);
	const focus = focusSummary(allItems, today, doneId);
	const list = sortMilestones(milestones);

	return (
		<div className="pm-ms-list">
			{(focus.doing.length > 0 || focus.dueToday.length > 0) && (
				<div className="pm-focus-strip">
					<span className="pm-focus-label">⚡ 今日聚焦</span>
					<span>
						{focus.doing.length} 个进行中
						{focus.dueToday.length > 0 &&
							" · " +
								focus.dueToday.length +
								" 个今日到期：「" +
								focus.dueToday[0].title +
								(focus.dueToday.length > 1 ? "」等" : "」")}
					</span>
					<button className="pm-chip pm-focus-go" onClick={onEnterBoard}>
						进入看板 →
					</button>
				</div>
			)}
			<div className="pm-legend" aria-hidden="true">
				<span>
					<i style={{ background: "var(--green)" }} />
					done
				</span>
				<span>
					<i style={{ background: "var(--accent)" }} />
					doing
				</span>
				<span>
					<i style={{ background: "var(--border)" }} />
					todo/backlog
				</span>
			</div>
			{!list.length && (
				<ResourceState
					title="还没有里程碑"
					detail="点击右上角「+ 新建里程碑」开始规划。"
				/>
			)}
			{list.map((m) => {
				const prog = milestoneProgress(items, m.id, doneId);
				const due = m.dueDate ? dueInfo(m.dueDate, today) : null;
				const overdue = Boolean(due?.overdue && m.status !== "closed");
				return (
					<article
						className={
							"pm-ms-row" +
							(overdue ? " overdue" : "") +
							(m.status === "closed" ? " closed" : "")
						}
						key={m.id}
					>
						<div className="pm-ms-top">
							<span className="pm-ms-name">🏁 {m.title}</span>
							{m.status === "closed" && <span className="pm-tag">已关闭</span>}
							{due && (
								<span className={"pm-ms-due" + (overdue ? " over" : "")}>
									📅 {due.short} · {due.text}
								</span>
							)}
							<span className="pm-ms-pct">
								{prog.done} / {prog.total}
								{prog.total > 0 && " · " + prog.pct + "%"}
							</span>
							<DropdownMenu>
								<DropdownMenuTrigger
									className="pm-ms-menu"
									aria-label={"里程碑操作：" + m.title}
								>
									⋯
								</DropdownMenuTrigger>
								<DropdownMenuContent>
									<DropdownMenuItem onSelect={() => onEdit(m)}>
										编辑里程碑…
									</DropdownMenuItem>
									<DropdownMenuItem onSelect={() => onDelete(m)}>
										删除里程碑…
									</DropdownMenuItem>
								</DropdownMenuContent>
							</DropdownMenu>
						</div>
						<div
							className="pm-bar"
							role="img"
							aria-label={
								"完成 " +
								prog.done +
								"，进行 " +
								prog.doing +
								"，待办 " +
								prog.rest
							}
						>
							{prog.done > 0 && (
								<i
									className="done"
									style={{ width: (prog.done / prog.total) * 100 + "%" }}
								/>
							)}
							{prog.doing > 0 && (
								<i
									className="doing"
									style={{ width: (prog.doing / prog.total) * 100 + "%" }}
								/>
							)}
							{prog.rest > 0 && (
								<i
									className="todo"
									style={{ width: (prog.rest / prog.total) * 100 + "%" }}
								/>
							)}
						</div>
						<div className="pm-ms-sub">
							{m.description && <span>{m.description}</span>}
							<span>
								doing {prog.doing} · todo/backlog {prog.rest}
							</span>
						</div>
					</article>
				);
			})}
		</div>
	);
}
