import { useState } from "react";
import type {
	PmItem,
	PmMilestoneWithStats,
	PmNewItem,
	PmStatusDef,
} from "../../types";
import { Button } from "../ui/button";
import {
	DialogContent,
	DialogDescription,
	DialogRoot,
	DialogTitle,
} from "../ui/dialog";
import { Input, Textarea } from "../ui/input";
import { Select } from "../ui/select";
import { PRIORITIES, PRIORITY_LABELS, repoName } from "./model";

const NONE = "__none__";

interface ItemDialogProps {
	/** null = 新建 */
	item: PmItem | null;
	/** 看板列「+ 添加」入口预填的状态 */
	presetStatus: string | null;
	statuses: PmStatusDef[];
	milestones: PmMilestoneWithStats[];
	repoOptions: string[];
	defaultRepoPath: string;
	onClose: () => void;
	onCreate: (input: PmNewItem) => Promise<void>;
	onUpdate: (item: PmItem) => Promise<void>;
	onDelete: (item: PmItem) => Promise<void>;
}

/** 新建 / 编辑任务；更新走 PUT 全字段语义，order/createdAt/githubRef 原样回传由服务端保留 */
export default function ItemDialog({
	item,
	presetStatus,
	statuses,
	milestones,
	repoOptions,
	defaultRepoPath,
	onClose,
	onCreate,
	onUpdate,
	onDelete,
}: ItemDialogProps) {
	const [title, setTitle] = useState(item?.title ?? "");
	const [body, setBody] = useState(item?.body ?? "");
	const [status, setStatus] = useState(
		item?.status ?? presetStatus ?? statuses[0]?.id ?? "backlog",
	);
	const [priority, setPriority] = useState(item?.priority ?? "none");
	const [milestoneId, setMilestoneId] = useState(item?.milestoneId ?? NONE);
	const [labels, setLabels] = useState((item?.labels ?? []).join(", "));
	const [repoPath, setRepoPath] = useState(item?.repoPath ?? NONE);
	const [branch, setBranch] = useState(item?.branch ?? "");
	const [dueDate, setDueDate] = useState(item?.dueDate ?? "");
	const [error, setError] = useState("");
	const [busy, setBusy] = useState(false);

	const run = async (action: () => Promise<void>) => {
		setBusy(true);
		setError("");
		try {
			await action();
			onClose();
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
		}
	};
	const submit = () =>
		run(async () => {
			const fields = {
				title: title.trim(),
				body,
				status,
				priority,
				milestoneId: milestoneId === NONE ? null : milestoneId,
				labels: labels
					.split(/[,，]/)
					.map((l) => l.trim())
					.filter(Boolean),
				repoPath: repoPath === NONE ? null : repoPath,
				branch: branch.trim() || null,
				dueDate: dueDate || null,
			};
			if (item) await onUpdate({ ...item, ...fields });
			else await onCreate(fields);
		});

	return (
		<DialogRoot open onOpenChange={(open) => !open && onClose()}>
			<DialogContent aria-label={item ? "编辑任务" : "新建任务"}>
				<DialogTitle>{item ? "编辑任务" : "新建任务"}</DialogTitle>
				<DialogDescription className="muted">
					{item ? "保存后写回本地数据库。" : "任务会进入所选看板列。"}
				</DialogDescription>
				{item?.githubRef && item.manualLock && (
					<div className="pm-lock-row">
						<span className="muted">
							🔒 已锁定：手动拖动过，同步不再自动迁移列位置。
						</span>
						<Button
							size="sm"
							disabled={busy}
							onClick={() =>
								run(() => onUpdate({ ...item, manualLock: false }))
							}
						>
							解除锁定，恢复自动迁移
						</Button>
					</div>
				)}
				<div className="pm-form">
					<label className="pm-field">
						<span>标题</span>
						<Input
							value={title}
							onChange={(e) => setTitle(e.target.value)}
							placeholder="要做什么？"
							required
							autoFocus
						/>
					</label>
					<label className="pm-field">
						<span>描述</span>
						<Textarea
							value={body}
							onChange={(e) => setBody(e.target.value)}
							rows={3}
							placeholder="可选，支持纯文本"
						/>
					</label>
					<div className="pm-field-row">
						<label className="pm-field">
							<span>状态</span>
							<Select
								aria-label="状态"
								value={status}
								onValueChange={setStatus}
								options={statuses.map((s) => ({
									value: s.id,
									label: s.name,
								}))}
							/>
						</label>
						<label className="pm-field">
							<span>优先级</span>
							<Select
								aria-label="优先级"
								value={priority}
								onValueChange={setPriority}
								options={PRIORITIES.map((p) => ({
									value: p,
									label: PRIORITY_LABELS[p],
								}))}
							/>
						</label>
					</div>
					<div className="pm-field-row">
						<label className="pm-field">
							<span>里程碑</span>
							<Select
								aria-label="里程碑"
								value={milestoneId}
								onValueChange={setMilestoneId}
								options={[
									{ value: NONE, label: "无" },
									...milestones.map((m) => ({
										value: m.id,
										label: m.title,
									})),
								]}
							/>
						</label>
						<label className="pm-field">
							<span>截止日</span>
							<Input
								type="date"
								value={dueDate}
								onChange={(e) => setDueDate(e.target.value)}
							/>
						</label>
					</div>
					<label className="pm-field">
						<span>标签（逗号分隔）</span>
						<Input
							value={labels}
							onChange={(e) => setLabels(e.target.value)}
							placeholder="infra, ui"
						/>
					</label>
					<div className="pm-field-row">
						<label className="pm-field">
							<span>仓库</span>
							<Select
								aria-label="关联仓库"
								value={repoPath}
								onValueChange={setRepoPath}
								options={[
									{ value: NONE, label: "无" },
									...repoOptions.map((r) => ({
										value: r,
										label:
											repoName(r) + (r === defaultRepoPath ? "（当前）" : ""),
									})),
								]}
							/>
						</label>
						<label className="pm-field">
							<span>分支</span>
							<Input
								value={branch}
								onChange={(e) => setBranch(e.target.value)}
								placeholder="可选"
							/>
						</label>
					</div>
					{error && (
						<div className="inline-error" role="alert">
							{error}
						</div>
					)}
					<div className="ops">
						{item && (
							<Button
								variant="danger"
								size="sm"
								disabled={busy}
								onClick={() => run(() => onDelete(item))}
							>
								删除任务
							</Button>
						)}
						<span style={{ flex: 1 }} />
						<Button onClick={onClose} disabled={busy}>
							取消
						</Button>
						<Button
							variant="primary"
							disabled={busy || !title.trim()}
							onClick={() => void submit()}
						>
							{busy ? "保存中…" : item ? "保存" : "创建"}
						</Button>
					</div>
				</div>
			</DialogContent>
		</DialogRoot>
	);
}
