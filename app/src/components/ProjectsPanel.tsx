import { openUrl } from "@tauri-apps/plugin-opener";
import { RefreshCw } from "lucide-react";
import { useState } from "react";
import * as api from "../api";
import { useStore } from "../store";
import type { Project } from "../types";
import IconButton from "./IconButton";
import { ExternalLink } from "./ProjectDetail";
import {
	isMissingProjectScope,
	listOwnerProjects,
	type ProjectV2Field,
	type ProjectV2Item,
	projectV2Details,
} from "./ProjectsV2";
import { ResourceState, useResource } from "./ResourceState";

const TOKENS_URL = "https://github.com/settings/tokens";

export default function ProjectsPanel({ p }: { p: Project }) {
	const gh = p.providerIdentity;
	const s = useStore();
	const projects = useResource(p.id + ":projects-v2", () =>
		gh ? listOwnerProjects(gh.owner) : Promise.resolve([]),
	);
	const [selected, setSelected] = useState<number | null>(null);
	const [view, setView] = useState<"table" | "board">("table");
	const current =
		projects.data?.find((x) => x.number === selected && !x.closed) ??
		projects.data?.find((x) => !x.closed) ??
		projects.data?.[0];
	const details = useResource(
		p.id +
			":projects-v2:" +
			(gh?.owner ?? "-") +
			":" +
			(current?.number ?? "-"),
		() =>
			gh && current
				? projectV2Details(gh.owner, current.number)
				: Promise.resolve(null),
	);
	if (!gh)
		return (
			<ResourceState
				title="此项目未关联 GitHub"
				detail="GitHub Projects 面板需要仓库具有 github.com 远程地址。"
			/>
		);
	const retry = () => {
		projects.reload();
		details.reload();
	};
	const scopeError = [projects.error, details.error].find((e) =>
		isMissingProjectScope(e),
	);
	if (scopeError) return <ReauthGuide onRetry={retry} />;
	if (projects.loading || projects.error)
		return (
			<ResourceState
				loading={projects.loading}
				error={projects.error}
				onRetry={projects.reload}
			/>
		);
	if (!projects.data?.length)
		return (
			<ResourceState
				title={gh.owner + " 还没有 Projects"}
				detail="GitHub Projects（V2）用于跨仓库的计划与看板管理，在 GitHub 网页端创建后会显示在这里。"
				action={
					<ExternalLink
						url={"https://github.com/" + gh.owner + "?tab=projects"}
					>
						前往 GitHub ↗
					</ExternalLink>
				}
			/>
		);
	const openItem = (item: ProjectV2Item) => {
		if (item.kind === "draft") return;
		const here = (gh.owner + "/" + gh.repo).toLowerCase();
		if (item.number !== null && item.repo?.toLowerCase() === here) {
			// 先切页签（setTab 会清空 workItemFocus），再设置聚焦目标。
			s.setTab(item.kind === "pr" ? "Pull Requests" : "Issues");
			s.setWorkItemFocus({
				pid: p.id,
				number: item.number,
				type: item.kind === "pr" ? "pr" : "issue",
			});
		} else if (item.url) {
			openUrl(item.url).catch((error) => s.toast(String(error)));
		}
	};
	return (
		<>
			<div className="page-heading">
				<div>
					<h2>Projects</h2>
					<p className="muted">
						只读查看 {gh.owner} 的 GitHub Projects（V2）；点击条目跳转对应 Issue
						/ PR。
					</p>
				</div>
				<IconButton
					label="刷新 Projects"
					icon={RefreshCw}
					busy={projects.loading || details.loading}
					onClick={retry}
				/>
			</div>
			<div className="list-tools">
				<select
					className="input"
					aria-label="选择 Project"
					value={current?.number ?? ""}
					onChange={(e) => setSelected(Number(e.target.value))}
				>
					{projects.data.map((x) => (
						<option key={x.id} value={x.number}>
							{x.title}
							{x.closed ? "（已关闭）" : ""}
						</option>
					))}
				</select>
				<div style={{ display: "flex", gap: 6 }}>
					<button
						className={"btn sm" + (view === "table" ? " primary" : "")}
						aria-pressed={view === "table"}
						onClick={() => setView("table")}
					>
						表格
					</button>
					<button
						className={"btn sm" + (view === "board" ? " primary" : "")}
						aria-pressed={view === "board"}
						onClick={() => setView("board")}
					>
						看板
					</button>
				</div>
				{current && (
					<ExternalLink url={current.url}>在 GitHub 打开 ↗</ExternalLink>
				)}
			</div>
			{details.loading || details.error ? (
				<ResourceState
					loading={details.loading}
					error={details.error}
					onRetry={details.reload}
				/>
			) : !details.data?.items.length ? (
				<ResourceState
					title="该 Project 暂无条目"
					detail="在 GitHub 网页端把 Issue / PR 加入 Project 后会显示在这里。"
				/>
			) : view === "table" ? (
				<TableView
					fields={details.data.fields}
					items={details.data.items}
					onOpen={openItem}
				/>
			) : (
				<BoardView
					fields={details.data.fields}
					items={details.data.items}
					onOpen={openItem}
					onUseTable={() => setView("table")}
				/>
			)}
		</>
	);
}

function KindBadge({ item }: { item: ProjectV2Item }) {
	if (item.kind === "draft") return <span className="badge">草稿</span>;
	return (
		<span className={"badge " + (item.kind === "pr" ? "b-pr" : "b-issue")}>
			{item.kind === "pr" ? "PR" : "Issue"}
		</span>
	);
}

function StateChip({ item }: { item: ProjectV2Item }) {
	if (item.kind === "draft" || !item.state) return null;
	const open = item.state === "open";
	return (
		<span className={"item-state " + (open ? "open" : "closed")}>
			{item.state === "merged" ? "已合并" : open ? "开放" : "已关闭"}
		</span>
	);
}

const thStyle: React.CSSProperties = {
	textAlign: "left",
	padding: "8px 12px",
	borderBottom: "1px solid var(--border)",
	color: "var(--muted)",
	fontWeight: 500,
	whiteSpace: "nowrap",
	position: "sticky",
	top: 0,
	background: "var(--panel)",
};
const tdStyle: React.CSSProperties = {
	padding: "8px 12px",
	borderBottom: "1px solid var(--border-soft)",
	verticalAlign: "top",
	maxWidth: 260,
	overflow: "hidden",
	textOverflow: "ellipsis",
	whiteSpace: "nowrap",
};

function TableView({
	fields,
	items,
	onOpen,
}: {
	fields: ProjectV2Field[];
	items: ProjectV2Item[];
	onOpen: (item: ProjectV2Item) => void;
}) {
	return (
		<div
			style={{
				overflow: "auto",
				border: "1px solid var(--border)",
				borderRadius: 8,
				maxHeight: "100%",
			}}
		>
			<table
				style={{
					borderCollapse: "collapse",
					width: "100%",
					fontSize: 13,
					background: "var(--panel)",
				}}
			>
				<thead>
					<tr>
						<th style={thStyle}>标题</th>
						{fields.map((f) => (
							<th key={f.id} style={thStyle}>
								{f.name}
							</th>
						))}
					</tr>
				</thead>
				<tbody>
					{items.map((item) => {
						const clickable = item.kind !== "draft";
						return (
							<tr
								key={item.id}
								onClick={clickable ? () => onOpen(item) : undefined}
								style={clickable ? { cursor: "pointer" } : undefined}
							>
								<td style={{ ...tdStyle, whiteSpace: "normal", minWidth: 220 }}>
									<KindBadge item={item} /> <StateChip item={item} />{" "}
									{item.title}{" "}
									<span className="muted">
										{item.number !== null ? "#" + item.number : ""}
										{item.repo ? " · " + item.repo : ""}
									</span>
								</td>
								{fields.map((f) => (
									<td
										key={f.id}
										style={tdStyle}
										title={item.fieldValues[f.name] ?? ""}
									>
										{item.fieldValues[f.name] ?? "—"}
									</td>
								))}
							</tr>
						);
					})}
				</tbody>
			</table>
		</div>
	);
}

function BoardView({
	fields,
	items,
	onOpen,
	onUseTable,
}: {
	fields: ProjectV2Field[];
	items: ProjectV2Item[];
	onOpen: (item: ProjectV2Item) => void;
	onUseTable: () => void;
}) {
	const status =
		fields.find(
			(f) =>
				f.dataType === "SINGLE_SELECT" && f.name.toLowerCase() === "status",
		) ?? fields.find((f) => f.dataType === "SINGLE_SELECT");
	if (!status)
		return (
			<ResourceState
				title="没有可用于看板的字段"
				detail="看板视图需要一个单选（Single Select）字段来分列，通常是 Status。可改用表格视图查看全部字段。"
				action={
					<button className="btn" onClick={onUseTable}>
						切换到表格视图
					</button>
				}
			/>
		);
	const NO_STATUS = "未设置";
	const columns = [...status.options.map((o) => o.name), NO_STATUS];
	const groups = new Map<string, ProjectV2Item[]>(columns.map((c) => [c, []]));
	for (const item of items) {
		const value = item.fieldValues[status.name];
		const key = value && groups.has(value) ? value : NO_STATUS;
		groups.get(key)?.push(item);
	}
	return (
		<div
			style={{ display: "flex", gap: 12, overflowX: "auto", paddingBottom: 8 }}
		>
			{columns.map((col) => {
				const list = groups.get(col) ?? [];
				if (col === NO_STATUS && !list.length) return null;
				return (
					<section
						key={col}
						aria-label={col}
						style={{
							minWidth: 240,
							width: 260,
							flexShrink: 0,
							border: "1px solid var(--border)",
							borderRadius: 8,
							background: "var(--panel)",
							display: "flex",
							flexDirection: "column",
							maxHeight: "100%",
						}}
					>
						<header
							style={{
								padding: "8px 10px",
								borderBottom: "1px solid var(--border-soft)",
								display: "flex",
								justifyContent: "space-between",
								color: "var(--muted)",
							}}
						>
							<strong style={{ color: "var(--text)" }}>{col}</strong>
							<span>{list.length}</span>
						</header>
						<div
							style={{
								padding: 8,
								display: "flex",
								flexDirection: "column",
								gap: 8,
								overflowY: "auto",
							}}
						>
							{list.map((item) => {
								const clickable = item.kind !== "draft";
								return (
									<button
										key={item.id}
										type="button"
										disabled={!clickable}
										onClick={() => onOpen(item)}
										style={{
											textAlign: "left",
											border: "1px solid var(--border-soft)",
											borderRadius: 6,
											padding: "8px 10px",
											background: "var(--panel-2)",
											cursor: clickable ? "pointer" : "default",
										}}
									>
										<div style={{ marginBottom: 4 }}>{item.title}</div>
										<div
											style={{
												display: "flex",
												gap: 6,
												alignItems: "center",
												flexWrap: "wrap",
											}}
										>
											<KindBadge item={item} />
											<StateChip item={item} />
											<span className="muted">
												{item.number !== null ? "#" + item.number : ""}
												{item.repo ? " · " + item.repo : ""}
											</span>
										</div>
									</button>
								);
							})}
							{!list.length && (
								<span className="muted" style={{ fontSize: 12 }}>
									暂无条目
								</span>
							)}
						</div>
					</section>
				);
			})}
		</div>
	);
}

function ReauthGuide({ onRetry }: { onRetry: () => void }) {
	const { reloadAuth, toast } = useStore();
	const [pat, setPat] = useState("");
	const [busy, setBusy] = useState(false);
	const submit = async () => {
		if (!pat.trim() || busy) return;
		setBusy(true);
		try {
			await api.loginPat(pat.trim());
			await reloadAuth();
			setPat("");
			toast("Token 已更新");
			onRetry();
		} catch (error) {
			toast("登录失败: " + String(error));
		} finally {
			setBusy(false);
		}
	};
	return (
		<div className="resource-state" role="status" style={{ maxWidth: 560 }}>
			<strong>需要重新授权才能读取 GitHub Projects</strong>
			<p>
				GitHub Projects（V2）使用独立的权限域，当前登录使用的 Token 缺少 project
				权限。Classic PAT 的权限在生成时固定、无法追加，因此需要到 GitHub
				重新生成一个勾选 project 权限的 Token。
			</p>
			<ol style={{ margin: 0, paddingLeft: 20, color: "var(--muted)" }}>
				<li>
					打开 GitHub 的 Token 设置页：
					<ExternalLink url={TOKENS_URL}>
						github.com/settings/tokens ↗
					</ExternalLink>
				</li>
				<li>
					选择 <b>Generate new token (classic)</b>，勾选 <b>repo</b>、
					<b>workflow</b> 和 <b>project</b> 三项权限。
				</li>
				<li>生成并复制新 Token，粘贴到下方保存；或回到登录流程重新登录。</li>
			</ol>
			<input
				className="input"
				type="password"
				aria-label="新的 Personal Access Token"
				placeholder="粘贴新的 classic PAT（ghp_…）"
				value={pat}
				onChange={(e) => setPat(e.target.value)}
				onKeyDown={(e) => e.key === "Enter" && submit()}
				style={{ alignSelf: "stretch" }}
			/>
			<div style={{ display: "flex", gap: 8 }}>
				<button
					className="btn primary"
					disabled={busy || !pat.trim()}
					onClick={submit}
				>
					{busy ? "验证中…" : "保存新 Token 并重试"}
				</button>
				<button className="btn" disabled={busy} onClick={onRetry}>
					已在别处更新授权，重试
				</button>
			</div>
		</div>
	);
}
