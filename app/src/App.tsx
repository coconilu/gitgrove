import { listen } from "@tauri-apps/api/event";
import { PanelLeft, PanelRight, Settings } from "lucide-react";
import { useEffect, useState } from "react";
import { addLocalFolder, CheckoutActions } from "./components/CheckoutActions";
import { Dialog, Toast } from "./components/Dialog";
import FilePreview from "./components/FilePreview";
import FileTreePanel from "./components/FileTreePanel";
import IconButton from "./components/IconButton";
import LoginScreen from "./components/LoginScreen";
import MyGitHub from "./components/MyGitHub";
import ProjectDetail, { ProjectTabs } from "./components/ProjectDetail";
import ResizeHandle from "./components/ResizeHandle";
import { ResourceState } from "./components/ResourceState";
import SettingsDialog from "./components/SettingsDialog";
import Sidebar from "./components/Sidebar";
import { UpdateToast } from "./components/UpdateControls";
import { activeCheckout, useStore } from "./store";
import { updates } from "./useUpdates";

export default function App() {
	const {
		auth,
		authLoaded,
		init,
		view,
		sel,
		projects,
		setCloneProgress,
		refreshCi,
		refreshStatusMap,
		layout,
		setLayout,
		projectsError,
		projectsLoading,
		refreshProjects,
	} = useStore();
	const [settingsOpen, setSettingsOpen] = useState(false);

	useEffect(() => {
		init();
		const un = listen<string>("clone-progress", (e) =>
			setCloneProgress(e.payload),
		);
		return () => {
			un.then((f) => f());
		};
	}, [init, setCloneProgress]);

	useEffect(() => {
		void updates.loadVersion();
		void updates.checkForUpdates();
		const timer = setInterval(
			() => void updates.checkForUpdates(),
			30 * 60 * 1000,
		);
		return () => clearInterval(timer);
	}, []);

	const selectedCheckout = activeCheckout(projects, sel);
	const selectedCid = selectedCheckout?.c.id;
	// 切换工作树或显式刷新项目后读取状态；仅切换同一工作树内的文件不重复请求。
	// biome-ignore lint/correctness/useExhaustiveDependencies: 以 checkout id 和项目快照作为刷新边界
	useEffect(() => {
		if (selectedCheckout) {
			refreshStatusMap(selectedCheckout.c);
			refreshCi(selectedCheckout.p);
		}
	}, [selectedCid, projects]);

	if (!authLoaded)
		return (
			<div className="app">
				<div className="login-wrap" style={{ color: "var(--muted)" }}>
					加载中…
				</div>
			</div>
		);

	if (!auth?.loggedIn)
		return (
			<div className="app">
				<LoginScreen />
				<IconButton
					label="打开设置"
					icon={Settings}
					type="button"
					className="login-settings"
					onClick={() => setSettingsOpen(true)}
				/>
				{settingsOpen && (
					<SettingsDialog onClose={() => setSettingsOpen(false)} />
				)}
				<Toast />
				<UpdateToast />
			</div>
		);

	const pid =
		sel?.kind === "project"
			? sel.pid
			: sel
				? activeCheckout(projects, sel)?.p.id
				: projects[0]?.id;
	const project = projects.find((x) => x.id === pid) ?? projects[0];

	const hit = activeCheckout(projects, sel);
	return (
		<div
			className="app"
			style={
				{
					"--sidebar-width": layout.sidebarWidth + "px",
					"--files-width": layout.filesWidth + "px",
				} as React.CSSProperties
			}
		>
			{layout.sidebarOpen && (
				<>
					<Sidebar />
					<ResizeHandle
						label="项目面板宽度"
						min={200}
						max={340}
						value={layout.sidebarWidth}
						onChange={(sidebarWidth) => setLayout({ sidebarWidth })}
					/>
				</>
			)}
			{layout.sidebarOpen && (
				<button
					className="sidebar-scrim"
					aria-label="关闭浮动项目面板"
					onClick={() => setLayout({ sidebarOpen: false })}
				/>
			)}
			<main className="main">
				<header className="workspace-header">
					<IconButton
						icon={PanelLeft}
						label={layout.sidebarOpen ? "收起项目面板" : "显示项目面板"}
						aria-expanded={layout.sidebarOpen}
						aria-controls="project-sidebar"
						onClick={() => setLayout({ sidebarOpen: !layout.sidebarOpen })}
					/>
					<div className="workspace-identity">
						<strong>
							{view === "github"
								? "GitHub 仓库"
								: (project?.name ?? "本地项目")}
						</strong>
						<span>
							{view === "github"
								? "查找仓库，开始本地工作"
								: hit
									? hit.c.branch +
										(hit.c.isPrimary ? " · 主工作区" : " · 工作树")
									: "添加项目开始工作"}
						</span>
					</div>
					{view === "projects" && hit && (
						<CheckoutActions p={hit.p} c={hit.c} />
					)}
					{view === "projects" && (
						<IconButton
							icon={PanelRight}
							label={layout.filesOpen ? "收起文件面板" : "显示文件面板"}
							disabled={!hit}
							aria-expanded={layout.filesOpen}
							aria-controls="file-panel"
							onClick={() => setLayout({ filesOpen: !layout.filesOpen })}
						/>
					)}
					<IconButton
						label="打开设置"
						icon={Settings}
						onClick={() => setSettingsOpen(true)}
					/>
				</header>
				{view === "github" ? (
					<MyGitHub />
				) : project ? (
					<>
						<ProjectTabs />
						{sel?.kind === "file" ? (
							<FilePreview fkey={sel.key} co={sel.co} />
						) : (
							<ProjectDetail key={project.id} p={project} />
						)}
					</>
				) : (
					<div className="content">
						<ResourceState
							loading={projectsLoading}
							error={projectsError}
							onRetry={() => refreshProjects()}
							title="还没有项目"
							detail="添加已有 Git 仓库，或从 GitHub 克隆一个仓库。"
							action={
								<button className="btn primary" onClick={addLocalFolder}>
									添加本地项目
								</button>
							}
						/>
					</div>
				)}
			</main>
			{view === "projects" && hit && layout.filesOpen && (
				<>
					<ResizeHandle
						label="文件面板宽度"
						min={240}
						max={420}
						value={layout.filesWidth}
						reverse
						onChange={(filesWidth) => setLayout({ filesWidth })}
					/>
					<button
						className="files-scrim"
						aria-label="关闭浮动文件面板"
						onClick={() => setLayout({ filesOpen: false })}
					/>
					<FileTreePanel />
				</>
			)}
			<Toast />
			<Dialog />
			{settingsOpen && (
				<SettingsDialog onClose={() => setSettingsOpen(false)} />
			)}
			<UpdateToast />
		</div>
	);
}
