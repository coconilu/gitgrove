import { RefreshCw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import { fileIconUrl, folderIconUrl } from "../fileIcons";
import { activeCheckout, useStore } from "../store";
import type { DirEntry } from "../types";
import IconButton from "./IconButton";
import { ResourceState } from "./ResourceState";

type Menu = {
	x: number;
	y: number;
	path: string;
	isDir: boolean;
	source: HTMLElement;
};
export default function FileTreePanel() {
	const {
		projects,
		sel,
		setSel,
		setLayout,
		statusMaps,
		statusErrors,
		refreshStatusMap,
		toast,
	} = useStore();
	const hit = activeCheckout(projects, sel);
	const cid = hit?.c.id ?? "";
	const path = hit?.c.path ?? "";
	const current = useRef(cid);
	current.current = cid;
	const [nodes, setNodes] = useState<Record<string, DirEntry[]>>({});
	const [expanded, setExpanded] = useState<Set<string>>(new Set());
	const [errors, setErrors] = useState<Record<string, string>>({});
	const [loading, setLoading] = useState(true);
	const [revision, setRevision] = useState(0);
	const [menu, setMenu] = useState<Menu | null>(null);
	const menuRef = useRef<HTMLDivElement>(null);
	// biome-ignore lint/correctness/useExhaustiveDependencies: revision 用于手动刷新目录
	useEffect(() => {
		let active = true;
		setNodes({});
		setErrors({});
		setLoading(true);
		setMenu(null);
		let saved: string[] = [];
		try {
			const raw = JSON.parse(
				localStorage.getItem("gh-projects.expanded." + cid) ?? "[]",
			);
			if (Array.isArray(raw)) saved = raw.filter((v) => typeof v === "string");
		} catch {
			/* 展开状态不可用时从根目录开始 */
		}
		setExpanded(new Set(saved));
		api.readDir(path).then(
			async (root) => {
				if (!active) return;
				setNodes({ "": root });
				setLoading(false);
				for (let i = 0; i < saved.length; i += 4)
					await Promise.all(
						saved.slice(i, i + 4).map(async (rel) => {
							try {
								const entries = await api.readDir(path + "/" + rel);
								if (active) setNodes((n) => ({ ...n, [rel]: entries }));
							} catch (e) {
								if (active) setErrors((v) => ({ ...v, [rel]: String(e) }));
							}
						}),
					);
			},
			(e) => {
				if (active) {
					setErrors({ "": String(e) });
					setLoading(false);
				}
			},
		);
		return () => {
			active = false;
		};
	}, [cid, path, revision]);
	useEffect(() => {
		if (!menu) return;
		menuRef.current?.querySelector("button")?.focus();
		const close = (e: PointerEvent) => {
			if (!menuRef.current?.contains(e.target as Node)) setMenu(null);
		};
		document.addEventListener("pointerdown", close);
		return () => document.removeEventListener("pointerdown", close);
	}, [menu]);
	if (!hit) return null;
	const smap = statusMaps[cid];
	const persist = (next: Set<string>) => {
		setExpanded(next);
		try {
			localStorage.setItem(
				"gh-projects.expanded." + cid,
				JSON.stringify([...next]),
			);
		} catch {
			/* 保留会话状态 */
		}
	};
	const readFolder = async (rel: string) => {
		const requestCid = cid;
		setErrors((e) => ({ ...e, [rel]: "" }));
		try {
			const list = await api.readDir(path + "/" + rel);
			if (current.current === requestCid)
				setNodes((n) => ({ ...n, [rel]: list }));
		} catch (e) {
			if (current.current === requestCid)
				setErrors((v) => ({ ...v, [rel]: String(e) }));
		}
	};
	const toggle = (rel: string) => {
		const next = new Set(expanded);
		if (next.has(rel)) next.delete(rel);
		else {
			next.add(rel);
			if (!nodes[rel]) void readFolder(rel);
		}
		persist(next);
	};
	const showMenu = (
		source: HTMLElement,
		abs: string,
		isDir: boolean,
		x?: number,
		y?: number,
	) => {
		const rect = source.getBoundingClientRect();
		setMenu({
			path: abs,
			isDir,
			source,
			x: Math.max(0, Math.min(x ?? rect.left, window.innerWidth - 210)),
			y: Math.max(0, Math.min(y ?? rect.bottom, window.innerHeight - 150)),
		});
	};
	const rows = (base: string, depth: number): React.ReactNode => {
		if (errors[base])
			return (
				<div className="folder-error" role="alert">
					{errors[base]}
					<button className="btn sm" onClick={() => readFolder(base)}>
						重试目录
					</button>
				</div>
			);
		if (!nodes[base])
			return (
				<div className="hint" role="status">
					读取目录…
				</div>
			);
		const list = nodes[base].filter((f) => f.name !== ".git");
		if (!list.length) return <div className="hint">空目录</div>;
		return list.map((f) => {
			const rel = base ? base + "/" + f.name : f.name;
			const abs = path + "/" + rel;
			const key = cid + "/" + rel;
			const ignored = smap?.ignored.some(
				(p) => rel === p || rel.startsWith(p + "/"),
			);
			const code = smap?.changes.find((c) => c.path === rel)?.code;
			const selected = sel?.kind === "file" && sel.key === key;
			const open = expanded.has(rel);
			return (
				<div key={rel}>
					<button
						role="treeitem"
						aria-expanded={f.isDir ? open : undefined}
						aria-selected={selected}
						className={
							"row file" +
							(selected ? " sel" : "") +
							(ignored ? " ignored" : "")
						}
						style={{ paddingLeft: 8 + depth * 16 }}
						title={rel}
						onClick={() =>
							f.isDir ? toggle(rel) : setSel({ kind: "file", key, co: cid })
						}
						onDoubleClick={() =>
							!f.isDir && api.openInEditor(abs).catch((e) => toast(String(e)))
						}
						onContextMenu={(e) => {
							e.preventDefault();
							showMenu(e.currentTarget, abs, f.isDir, e.clientX, e.clientY);
						}}
						onKeyDown={(e) => {
							if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
								e.preventDefault();
								showMenu(e.currentTarget, abs, f.isDir);
							}
							if (
								f.isDir &&
								((e.key === "ArrowRight" && !open) ||
									(e.key === "ArrowLeft" && open))
							) {
								e.preventDefault();
								toggle(rel);
							}
							if (["ArrowUp", "ArrowDown", "Home", "End"].includes(e.key)) {
								e.preventDefault();
								const items = Array.from(
									e.currentTarget
										.closest('[role="tree"]')!
										.querySelectorAll<HTMLButtonElement>('[role="treeitem"]'),
								);
								const index = items.indexOf(e.currentTarget);
								items[
									e.key === "Home"
										? 0
										: e.key === "End"
											? items.length - 1
											: Math.max(
													0,
													Math.min(
														items.length - 1,
														index + (e.key === "ArrowDown" ? 1 : -1),
													),
												)
								]?.focus();
							}
						}}
					>
						<span className="twisty" aria-hidden="true">
							{f.isDir ? (open ? "⌄" : "›") : ""}
						</span>
						<img
							className="ficon"
							src={f.isDir ? folderIconUrl(f.name, open) : fileIconUrl(f.name)}
							alt=""
							draggable={false}
						/>
						<span className="fname">{f.name}</span>
						{code && (
							<span className={"gs " + code} title={"Git 状态 " + code}>
								{code}
							</span>
						)}
					</button>
					{f.isDir && open && (
						// biome-ignore lint/a11y/useSemanticElements: 文件树的嵌套组不是表单字段集
						<div role="group">{rows(rel, depth + 1)}</div>
					)}
				</div>
			);
		});
	};
	const menuAction = async (action: () => Promise<unknown>) => {
		try {
			await action();
		} catch (e) {
			toast(String(e));
		}
		menu?.source.focus();
		setMenu(null);
	};
	return (
		<aside id="file-panel" className="ctx" aria-label="当前工作树文件">
			<div className="file-panel-heading">
				<strong>文件</strong>
				<div className="ops">
					<IconButton
						label="刷新文件"
						icon={RefreshCw}
						busy={loading}
						onClick={() => {
							setRevision((n) => n + 1);
							void refreshStatusMap(hit.c);
						}}
					/>
					<IconButton
						label="关闭文件面板"
						icon={X}
						onClick={() => setLayout({ filesOpen: false })}
					/>
				</div>
			</div>
			<div className="file-root path-text">{path}</div>
			{statusErrors[cid] && (
				<div className="folder-error" role="alert">
					文件 Git 状态读取失败
					<button className="btn sm" onClick={() => refreshStatusMap(hit.c)}>
						重试
					</button>
				</div>
			)}
			<div className="ctx-tree">
				{loading || errors[""] ? (
					<ResourceState
						loading={loading}
						error={errors[""]}
						onRetry={() => setRevision((n) => n + 1)}
					/>
				) : (
					<div role="tree" aria-label={"文件树：" + hit.c.branch}>
						{rows("", 0)}
					</div>
				)}
			</div>
			<div className="file-panel-foot">单击预览 · 双击用编辑器打开</div>
			{menu && (
				<div
					ref={menuRef}
					className="ctx-menu"
					role="menu"
					aria-label="文件操作"
					style={{ left: menu.x, top: menu.y }}
					onKeyDown={(e) => {
						if (e.key === "Escape") {
							e.preventDefault();
							menu.source.focus();
							setMenu(null);
						}
						if (e.key === "ArrowDown" || e.key === "ArrowUp") {
							e.preventDefault();
							const buttons = Array.from(
								e.currentTarget.querySelectorAll("button"),
							);
							const idx = buttons.indexOf(
								document.activeElement as HTMLButtonElement,
							);
							buttons[
								(idx + (e.key === "ArrowDown" ? 1 : buttons.length - 1)) %
									buttons.length
							]?.focus();
						}
					}}
				>
					<button
						role="menuitem"
						onClick={() =>
							menuAction(async () => {
								await navigator.clipboard.writeText(menu.path);
								toast("路径已复制");
							})
						}
					>
						复制路径
					</button>
					<button
						role="menuitem"
						onClick={() => menuAction(() => api.revealInExplorer(menu.path))}
					>
						在文件管理器显示
					</button>
					{!menu.isDir && (
						<button
							role="menuitem"
							onClick={() => menuAction(() => api.openInEditor(menu.path))}
						>
							在编辑器打开
						</button>
					)}
				</div>
			)}
		</aside>
	);
}
