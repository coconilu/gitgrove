import { useEffect, useRef, useState } from "react";
import { type DialogState, useStore } from "../store";
import { Button } from "./ui/button";
import {
	DialogContent,
	DialogDescription,
	DialogRoot,
	DialogTitle,
} from "./ui/dialog";
import { Input, Textarea } from "./ui/input";
import { Select } from "./ui/select";

export function Toast() {
	const msg = useStore((s) => s.toastMsg);
	return (
		<div
			className={"toast" + (msg ? " show" : "")}
			role="status"
			aria-live="polite"
		>
			{msg}
		</div>
	);
}
export function Dialog() {
	const dialog = useStore((s) => s.dialog);
	// 关闭动画播完前保留最后一份配置，避免内容提前卸载。
	const [shown, setShown] = useState<DialogState | null>(dialog);
	useEffect(() => {
		if (dialog) setShown(dialog);
	}, [dialog]);
	if (!shown) return null;
	return <DialogBody key={shown.title} open={!!dialog} dialog={shown} />;
}
function DialogBody({ open, dialog }: { open: boolean; dialog: DialogState }) {
	const contentRef = useRef<HTMLDivElement>(null);
	const busyRef = useRef(false);
	const [value, setValue] = useState(dialog.defaultValue ?? "");
	const [values, setValues] = useState<Record<string, string>>(
		Object.fromEntries(
			dialog.fields?.map((f) => [f.name, f.defaultValue ?? ""]) ?? [],
		),
	);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState("");
	const close = useStore((s) => s.closeDialog);
	const execute = async (secondary = false) => {
		if (busyRef.current) return;
		const validation = !secondary && dialog.validate?.(value, values);
		if (validation) {
			setError(validation);
			return;
		}
		busyRef.current = true;
		setBusy(true);
		setError("");
		try {
			if (secondary) await dialog.onSecondary?.();
			else await dialog.onSubmit(value, values);
			// 回调可能打开下一步对话框，不能把新对话框关掉。
			if (useStore.getState().dialog === dialog) close();
		} catch (e) {
			setError(String(e));
		} finally {
			busyRef.current = false;
			setBusy(false);
		}
	};
	return (
		<DialogRoot
			open={open}
			onOpenChange={(next) => {
				if (!next && !busyRef.current) close();
			}}
		>
			<DialogContent
				ref={contentRef}
				onOpenAutoFocus={(e) => {
					// 危险操作默认聚焦取消按钮，其余交给 Radix 聚焦首个可交互控件。
					if (!dialog.danger) return;
					e.preventDefault();
					contentRef.current
						?.querySelector<HTMLElement>(".cancel-btn")
						?.focus();
				}}
				onPointerDownOutside={(e) => e.preventDefault()}
				onInteractOutside={(e) => e.preventDefault()}
			>
				<form
					onSubmit={(e) => {
						e.preventDefault();
						void execute();
					}}
				>
					<DialogTitle>{dialog.title}</DialogTitle>
					<DialogDescription className="msg">
						{dialog.message}
					</DialogDescription>
					{dialog.kind === "prompt" && !dialog.fields && (
						<label className="field">
							<span>{dialog.placeholder ?? "输入内容"}</span>
							<Input
								required
								value={value}
								disabled={busy}
								placeholder={dialog.placeholder}
								onChange={(e) => setValue(e.target.value)}
							/>
						</label>
					)}
					{dialog.fields?.map((f) => (
						<label className="field" key={f.name}>
							<span>
								{f.label}
								{f.required ? " *" : ""}
							</span>
							{f.type === "select" ? (
								<Select
									name={f.name}
									value={values[f.name]}
									disabled={busy}
									required={f.required}
									placeholder={f.defaultValue ? undefined : "请选择"}
									options={(f.options ?? []).map((o) => ({
										value: o,
										label: o,
									}))}
									onValueChange={(v) =>
										setValues((prev) => ({ ...prev, [f.name]: v }))
									}
								/>
							) : f.type === "textarea" ? (
								<Textarea
									value={values[f.name]}
									disabled={busy}
									required={f.required}
									onChange={(e) =>
										setValues((v) => ({ ...v, [f.name]: e.target.value }))
									}
								/>
							) : (
								<Input
									type={f.type ?? "text"}
									value={values[f.name]}
									disabled={busy}
									required={f.required}
									onChange={(e) =>
										setValues((v) => ({ ...v, [f.name]: e.target.value }))
									}
								/>
							)}
							{f.help && <small>{f.help}</small>}
						</label>
					))}
					{dialog.describe && (
						<div className="operation-preview">
							{dialog.describe(value, values)}
						</div>
					)}
					{error && (
						<div className="inline-error" role="alert">
							{error}
						</div>
					)}
					<div className="ops">
						<Button className="cancel-btn" disabled={busy} onClick={close}>
							取消
						</Button>
						{dialog.secondaryText && (
							<Button disabled={busy} onClick={() => void execute(true)}>
								{dialog.secondaryText}
							</Button>
						)}
						<Button
							type="submit"
							variant={dialog.danger ? "danger" : "primary"}
							disabled={busy}
						>
							{busy
								? "处理中…"
								: (dialog.okText ??
									(dialog.kind === "confirm" ? "确认操作" : "保存"))}
						</Button>
					</div>
				</form>
			</DialogContent>
		</DialogRoot>
	);
}
