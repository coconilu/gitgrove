import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../api";
import { useStore } from "../store";
import type { LaunchApp, LaunchPreferences } from "../types";
import { AboutPanel } from "./UpdateControls";

export default function SettingsDialog({ onClose }: { onClose: () => void }) {
	const ref = useRef<HTMLDialogElement>(null);
	const [panel, setPanel] = useState<"general" | "about">("general");
	const busyRef = useRef(false);
	const mounted = useRef(false);
	const detecting = useRef(false);
	const [preferences, setPreferences] = useState<LaunchPreferences | null>(
		null,
	);
	const [apps, setApps] = useState<LaunchApp[]>([]);
	const [scanning, setScanning] = useState(false);
	const [scanError, setScanError] = useState("");
	const [error, setError] = useState("");
	const [busy, setBusy] = useState(false);
	const toast = useStore((s) => s.toast);

	const scan = useCallback(async () => {
		if (detecting.current) return;
		detecting.current = true;
		setScanning(true);
		try {
			const result = await api.discoverLaunchApps();
			if (mounted.current) {
				setApps(result);
				setScanError("");
			}
		} catch (e) {
			if (mounted.current) setScanError(String(e));
		} finally {
			detecting.current = false;
			if (mounted.current) setScanning(false);
		}
	}, []);

	const loadPreferences = useCallback(async () => {
		try {
			const result = await api.getLaunchPreferences();
			if (mounted.current) {
				setPreferences(result);
				setError("");
			}
		} catch (e) {
			if (mounted.current) setError(String(e));
		}
	}, []);

	useEffect(() => {
		mounted.current = true;
		const previous = document.activeElement as HTMLElement | null;
		const modal = ref.current!;
		modal.showModal();
		void loadPreferences();
		void scan();
		const refresh = () => {
			if (document.visibilityState === "visible") void scan();
		};
		const timer = setInterval(refresh, 5000);
		window.addEventListener("focus", refresh);
		return () => {
			mounted.current = false;
			clearInterval(timer);
			window.removeEventListener("focus", refresh);
			modal.close();
			previous?.focus();
		};
	}, [loadPreferences, scan]);

	const save = async () => {
		if (!preferences || busyRef.current) return;
		busyRef.current = true;
		setBusy(true);
		setError("");
		try {
			await api.saveLaunchPreferences(preferences);
			toast("配置已保存，下次打开编辑器或终端时生效");
			onClose();
		} catch (e) {
			setError(String(e));
		} finally {
			busyRef.current = false;
			if (mounted.current) setBusy(false);
		}
	};

	const choiceGroup = (kind: "editor" | "terminal", title: string) => {
		const options = apps.filter((app) => app.kind === kind);
		const selected = preferences?.[kind] ?? "auto";
		const automatic = options.find((app) => app.path);
		return (
			<fieldset className="launch-section" disabled={busy || !preferences}>
				<legend>{title}</legend>
				<label className="launch-auto">
					<input
						type="radio"
						name={kind}
						value="auto"
						checked={selected === "auto"}
						onChange={() =>
							setPreferences((p) => p && { ...p, [kind]: "auto" })
						}
					/>
					<span>自动选择</span>
					<small>
						{automatic
							? "当前使用 " + automatic.name
							: scanning
								? "检测中…"
								: "未检测到可用应用"}
					</small>
				</label>
				<div className={"launch-options " + kind}>
					{options.map((app) => (
						<label
							className={
								"launch-option" +
								(selected === app.id ? " selected" : "") +
								(!app.path ? " unavailable" : "")
							}
							key={app.id}
						>
							<input
								type="radio"
								name={kind}
								value={app.id}
								aria-label={app.name}
								checked={selected === app.id}
								disabled={!app.path}
								onChange={() =>
									setPreferences((p) => p && { ...p, [kind]: app.id })
								}
							/>
							<span className="launch-option-content">
								<span className="launch-option-heading">
									<strong>{app.name}</strong>
									<span
										className={
											"launch-status" + (app.running ? " running" : "")
										}
									>
										{app.running
											? "运行中"
											: !app.path
												? "未检测到"
												: app.running === null
													? "进程状态未知"
													: "已安装"}
									</span>
								</span>
								<small className="launch-path" title={app.path ?? undefined}>
									{app.path ?? "安装后点击“重新检测”"}
								</small>
							</span>
						</label>
					))}
				</div>
				{selected !== "auto" &&
					!options.some((app) => app.id === selected && app.path) &&
					!scanning && (
						<p className="inline-error" role="alert">
							已保存的选择当前不可用，请重新选择或切换为自动。
						</p>
					)}
			</fieldset>
		);
	};

	return (
		<dialog
			ref={ref}
			className="dialog settings-dialog"
			aria-labelledby="settings-title"
			aria-describedby="settings-description"
			onCancel={(e) => {
				e.preventDefault();
				if (!busyRef.current) onClose();
			}}
		>
			<h3 id="settings-title">配置</h3>
			<p id="settings-description" className="msg">
				管理默认打开方式与应用更新。
			</p>
			<nav className="settings-nav" aria-label="配置分组">
				<button
					type="button"
					className="btn"
					aria-pressed={panel === "general"}
					disabled={busy}
					onClick={() => setPanel("general")}
				>
					常规
				</button>
				<button
					type="button"
					className="btn"
					aria-pressed={panel === "about"}
					disabled={busy}
					onClick={() => setPanel("about")}
				>
					关于
				</button>
			</nav>
			<form
				hidden={panel !== "general"}
				onSubmit={(e) => {
					e.preventDefault();
					void save();
				}}
			>
				<div className="settings-heading">
					<div>
						<h4>默认打开方式</h4>
						<p className="msg">设置所有项目的默认打开方式。</p>
					</div>
					<button
						type="button"
						className="btn sm"
						disabled={scanning || busy}
						onClick={() => void scan()}
					>
						{scanning ? "检测中…" : "重新检测"}
					</button>
				</div>
				{choiceGroup("editor", "默认编辑器")}
				{choiceGroup("terminal", "默认终端")}
				<p className="settings-hint">
					运行状态每 5
					秒刷新，仅在此弹窗打开时检测。选择终端后会在项目目录创建新会话；“运行中”表示检测到对应进程。
				</p>
				{scanError && (
					<p className="inline-error" role="alert">
						检测失败：{scanError}
					</p>
				)}
				{error && (
					<p className="inline-error" role="alert">
						{error}
					</p>
				)}
				{!preferences && error && (
					<button
						type="button"
						className="btn"
						onClick={() => void loadPreferences()}
					>
						重试读取配置
					</button>
				)}
				<div className="ops">
					<button
						type="button"
						className="btn"
						disabled={busy}
						onClick={onClose}
					>
						取消
					</button>
					<button
						type="submit"
						className="btn primary"
						disabled={busy || !preferences}
					>
						{busy ? "保存中…" : "保存配置"}
					</button>
				</div>
			</form>
			{panel === "about" && (
				<>
					<AboutPanel />
					<div className="ops">
						<button
							type="button"
							className="btn"
							disabled={busy}
							onClick={onClose}
						>
							关闭
						</button>
					</div>
				</>
			)}
		</dialog>
	);
}
