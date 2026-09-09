import { ArrowDown, ArrowUp, Trash2 } from "lucide-react";
import { useState } from "react";
import type { PmItem, PmStatusDef } from "../../types";
import IconButton from "../IconButton";
import { Button } from "../ui/button";
import {
	DialogContent,
	DialogDescription,
	DialogRoot,
	DialogTitle,
} from "../ui/dialog";
import { Input } from "../ui/input";

interface ColumnsDialogProps {
	statuses: PmStatusDef[];
	items: PmItem[];
	onClose: () => void;
	onSave: (statuses: PmStatusDef[]) => Promise<void>;
}

function slugify(name: string, taken: Set<string>): string {
	const base =
		name
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, "-")
			.replace(/^-|-$/g, "") || "col";
	let id = base;
	for (let n = 2; taken.has(id); n++) id = base + "-" + n;
	return id;
}

/** 看板列管理：增删改 + 排序；保存时整表提交给 pm_update_statuses */
export default function ColumnsDialog({
	statuses,
	items,
	onClose,
	onSave,
}: ColumnsDialogProps) {
	const [cols, setCols] = useState<PmStatusDef[]>(statuses);
	const [newName, setNewName] = useState("");
	const [error, setError] = useState("");
	const [busy, setBusy] = useState(false);

	const countIn = (id: string) => items.filter((i) => i.status === id).length;
	const rename = (id: string, name: string) =>
		setCols((list) => list.map((c) => (c.id === id ? { ...c, name } : c)));
	const move = (id: string, dir: -1 | 1) =>
		setCols((list) => {
			const at = list.findIndex((c) => c.id === id);
			const to = at + dir;
			if (at < 0 || to < 0 || to >= list.length) return list;
			const next = [...list];
			[next[at], next[to]] = [next[to], next[at]];
			return next;
		});
	const remove = (id: string) => {
		if (cols.length <= 1) {
			setError("至少保留一列");
			return;
		}
		if (countIn(id) > 0) {
			setError("该列还有任务，请先移走或删除这些任务");
			return;
		}
		setError("");
		setCols((list) => list.filter((c) => c.id !== id));
	};
	const add = () => {
		const name = newName.trim();
		if (!name) return;
		const id = slugify(name, new Set(cols.map((c) => c.id)));
		setCols((list) => [...list, { id, name }]);
		setNewName("");
		setError("");
	};
	const save = async () => {
		if (cols.some((c) => !c.name.trim())) {
			setError("列名不能为空");
			return;
		}
		setBusy(true);
		setError("");
		try {
			await onSave(cols.map((c) => ({ ...c, name: c.name.trim() })));
			onClose();
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
		}
	};

	return (
		<DialogRoot open onOpenChange={(open) => !open && onClose()}>
			<DialogContent aria-label="看板列管理">
				<DialogTitle>看板列管理</DialogTitle>
				<DialogDescription className="muted">
					最后一列视为「完成」列，里程碑进度按它统计。
				</DialogDescription>
				<div className="pm-form">
					{cols.map((c, idx) => (
						<div className="pm-col-row" key={c.id}>
							<Input
								value={c.name}
								aria-label={"列名：" + c.name}
								onChange={(e) => rename(c.id, e.target.value)}
							/>
							<span className="pm-count">{countIn(c.id)}</span>
							<IconButton
								label="上移"
								icon={ArrowUp}
								disabled={idx === 0}
								onClick={() => move(c.id, -1)}
							/>
							<IconButton
								label="下移"
								icon={ArrowDown}
								disabled={idx === cols.length - 1}
								onClick={() => move(c.id, 1)}
							/>
							<IconButton
								label={"删除列 " + c.name}
								icon={Trash2}
								disabled={busy}
								onClick={() => remove(c.id)}
							/>
						</div>
					))}
					<div className="pm-col-row">
						<Input
							value={newName}
							aria-label="新列名"
							placeholder="新列名，如 Review"
							onChange={(e) => setNewName(e.target.value)}
							onKeyDown={(e) => e.key === "Enter" && add()}
						/>
						<Button size="sm" onClick={add} disabled={!newName.trim()}>
							添加列
						</Button>
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
							disabled={busy}
							onClick={() => void save()}
						>
							{busy ? "保存中…" : "保存"}
						</Button>
					</div>
				</div>
			</DialogContent>
		</DialogRoot>
	);
}
