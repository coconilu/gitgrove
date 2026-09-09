import { useState } from "react";
import type { PmMilestoneWithStats } from "../../types";
import { Button } from "../ui/button";
import {
	DialogContent,
	DialogDescription,
	DialogRoot,
	DialogTitle,
} from "../ui/dialog";
import { Input, Textarea } from "../ui/input";
import { Select } from "../ui/select";

interface MilestoneDialogProps {
	/** null = 新建 */
	editing: PmMilestoneWithStats | null;
	onClose: () => void;
	onSave: (
		editing: PmMilestoneWithStats | null,
		title: string,
		description: string,
		dueDate: string | null,
		status: string,
	) => Promise<void>;
}

export default function MilestoneDialog({
	editing,
	onClose,
	onSave,
}: MilestoneDialogProps) {
	const [title, setTitle] = useState(editing?.title ?? "");
	const [description, setDescription] = useState(editing?.description ?? "");
	const [dueDate, setDueDate] = useState(editing?.dueDate ?? "");
	const [status, setStatus] = useState(editing?.status ?? "open");
	const [error, setError] = useState("");
	const [busy, setBusy] = useState(false);

	const submit = async () => {
		setBusy(true);
		setError("");
		try {
			await onSave(editing, title.trim(), description, dueDate || null, status);
			onClose();
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
		}
	};

	return (
		<DialogRoot open onOpenChange={(open) => !open && onClose()}>
			<DialogContent aria-label={editing ? "编辑里程碑" : "新建里程碑"}>
				<DialogTitle>{editing ? "编辑里程碑" : "新建里程碑"}</DialogTitle>
				<DialogDescription className="muted">
					进度由里程碑下任务的状态自动聚合。
				</DialogDescription>
				<div className="pm-form">
					<label className="pm-field">
						<span>标题</span>
						<Input
							value={title}
							onChange={(e) => setTitle(e.target.value)}
							placeholder="例如：v1.2 自研项目管理"
							required
							autoFocus
						/>
					</label>
					<label className="pm-field">
						<span>描述</span>
						<Textarea
							value={description}
							onChange={(e) => setDescription(e.target.value)}
							rows={3}
							placeholder="可选"
						/>
					</label>
					<div className="pm-field-row">
						<label className="pm-field">
							<span>截止日</span>
							<Input
								type="date"
								value={dueDate}
								onChange={(e) => setDueDate(e.target.value)}
							/>
						</label>
						{editing && (
							<label className="pm-field">
								<span>状态</span>
								<Select
									aria-label="里程碑状态"
									value={status}
									onValueChange={setStatus}
									options={[
										{ value: "open", label: "开放" },
										{ value: "closed", label: "已关闭" },
									]}
								/>
							</label>
						)}
					</div>
					{error && (
						<div className="inline-error" role="alert">
							{error}
						</div>
					)}
					<div className="ops">
						<span style={{ flex: 1 }} />
						<Button onClick={onClose} disabled={busy}>
							取消
						</Button>
						<Button
							variant="primary"
							disabled={busy || !title.trim()}
							onClick={() => void submit()}
						>
							{busy ? "保存中…" : editing ? "保存" : "创建"}
						</Button>
					</div>
				</div>
			</DialogContent>
		</DialogRoot>
	);
}
