import { useEffect, useRef, useState } from "react";
import { create } from "zustand";
import * as api from "../api";
import { useStore } from "../store";

type Destination = "codex" | "kimi" | "editor" | "terminal";
const labels: Record<Destination, string> = {
	codex: "Codex",
	kimi: "Kimi Code",
	editor: "默认编辑器",
	terminal: "默认终端",
};
// Shared across the header and repository list, including component remounts.
const useOpening = create<{ target: Destination | null }>(() => ({
	target: null,
}));
let supportRequest: Promise<boolean> | undefined;
function supportsAgents() {
	supportRequest ??= api.agentOpenSupport().catch((error) => {
		supportRequest = undefined;
		throw error;
	});
	return supportRequest;
}

export function OpenInMenu({ path }: { path: string }) {
	const target = useOpening((s) => s.target);
	const [open, setOpen] = useState(false);
	const [supported, setSupported] = useState<boolean | null>(null);
	const [error, setError] = useState("");
	const container = useRef<HTMLDivElement>(null);
	const trigger = useRef<HTMLButtonElement>(null);
	useEffect(() => {
		if (!open) return;
		setError("");
		let cancelled = false;
		void supportsAgents().then(
			(value) => !cancelled && setSupported(value),
			() => !cancelled && setError("无法检查工具支持，请关闭菜单后重试。"),
		);
		const closeOutside = (event: PointerEvent) => {
			if (!container.current?.contains(event.target as Node)) setOpen(false);
		};
		document.addEventListener("pointerdown", closeOutside);
		return () => {
			cancelled = true;
			document.removeEventListener("pointerdown", closeOutside);
		};
	}, [open]);
	const launch = async (destination: Destination) => {
		if (useOpening.getState().target !== null) return;
		useOpening.setState({ target: destination });
		setError("");
		try {
			if (destination === "editor") await api.openInEditor(path);
			else if (destination === "terminal") await api.openInTerminal(path);
			else {
				const result = await api.openInAgent(path, destination);
				useStore.getState().toast(result.message);
			}
			setOpen(false);
			trigger.current?.focus();
		} catch (error) {
			setError(String(error));
			// Navigation may have unmounted the originating menu during startup.
			useStore.getState().toast(String(error));
			// Disabling the clicked option can drop focus to body. Restore this
			// menu's keyboard path without taking focus from another control/modal.
			if (
				document.activeElement === document.body &&
				!document.querySelector("dialog[open]") &&
				trigger.current?.getAttribute("aria-expanded") === "true"
			) {
				trigger.current.focus();
			}
		} finally {
			useOpening.setState({ target: null });
		}
	};
	return (
		<div
			className="more-menu open-in-menu"
			ref={container}
			onKeyDown={(event) => {
				if (event.key === "Escape") {
					setOpen(false);
					trigger.current?.focus();
				}
			}}
		>
			<button
				className="btn primary"
				ref={trigger}
				aria-expanded={open}
				title={path}
				onClick={() => setOpen(!open)}
			>
				{target ? `正在打开 ${labels[target]}…` : "在…中打开"}
			</button>
			{open && (
				<div
					className="more-popover open-in-popover"
					aria-busy={target !== null}
				>
					{(Object.keys(labels) as Destination[]).map((destination) => (
						<button
							key={destination}
							disabled={
								target !== null ||
								((destination === "codex" || destination === "kimi") &&
									supported !== true)
							}
							onClick={() => void launch(destination)}
						>
							在 {labels[destination]} 中打开
						</button>
					))}
					{supported === false && (
						<p className="muted">Codex / Kimi Code 暂仅支持 Windows。</p>
					)}
					{supported === null && !error && (
						<p className="muted">检查工具支持…</p>
					)}
					{error && (
						<p className="inline-error" role="alert">
							{error}
						</p>
					)}
				</div>
			)}
		</div>
	);
}
