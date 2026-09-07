import { invoke } from "@tauri-apps/api/core";
import type {
	AuthState,
	BranchInfo,
	CheckoutInfo,
	CloneTargetInfo,
	DirEntry,
	FilePreview,
	IssueInfo,
	LaunchApp,
	LaunchPreferences,
	LinkedWorkItem,
	PrInfo,
	Project,
	RepoInfo,
	RunInfo,
	StatusMap,
	WorkflowDetails,
	WorkflowInfo,
} from "./types";

// ---- 认证 ----
export const authStatus = () => invoke<AuthState>("auth_status");
export const loginPat = (token: string) =>
	invoke<AuthState>("login_pat", { token });
export const logout = () => invoke<void>("logout");

// ---- My GitHub ----
export const listMyRepos = () => invoke<RepoInfo[]>("list_my_repos");

// ---- clone / 项目 ----
export const checkCloneTarget = (repo: string) =>
	invoke<CloneTargetInfo>("check_clone_target", { repo });
export const cloneRepo = (url: string, repo: string) =>
	invoke<Project>("clone_repo", { url, repo });
export const addExistingProject = (path: string) =>
	invoke<Project>("add_existing_project", { path });
export const listProjects = () => invoke<Project[]>("list_projects");
export const removeProject = (projectId: string, deleteFiles: boolean) =>
	invoke<void>("remove_project", { projectId, deleteFiles });

// ---- worktree / 分支 ----
export const listBranches = (projectId: string) =>
	invoke<BranchInfo[]>("list_branches", { projectId });
export const createWorktree = (
	projectId: string,
	branch: string,
	base: string | null,
	createBranch: boolean,
	linked: LinkedWorkItem | null,
) =>
	invoke<CheckoutInfo>("create_worktree", {
		projectId,
		branch,
		base,
		createBranch,
		linked,
	});
export const removeWorktree = (
	projectId: string,
	path: string,
	toTrash: boolean,
) => invoke<void>("remove_worktree", { projectId, path, toTrash });
export const lockWorktree = (path: string, lock: boolean) =>
	invoke<void>("lock_worktree", { path, lock });

// ---- 文件系统 ----
export const readDir = (path: string) =>
	invoke<DirEntry[]>("read_dir", { path });
export const readFilePreview = (path: string) =>
	invoke<FilePreview>("read_file_preview", { path });
export const checkoutStatus = (path: string) =>
	invoke<StatusMap>("checkout_status", { path });
export const openInEditor = (path: string) =>
	invoke<void>("open_in_editor", { path });
export const openInTerminal = (path: string) =>
	invoke<void>("open_in_terminal", { path });
export const agentOpenSupport = () => invoke<boolean>("agent_open_support");
export const openInAgent = (path: string, agent: "codex" | "kimi") =>
	invoke<{ message: string }>("open_in_agent", { path, agent });
export const getLaunchPreferences = () =>
	invoke<LaunchPreferences>("get_launch_preferences");
export const saveLaunchPreferences = (preferences: LaunchPreferences) =>
	invoke<LaunchPreferences>("save_launch_preferences", { preferences });
export const discoverLaunchApps = () =>
	invoke<LaunchApp[]>("discover_launch_apps");
export const revealInExplorer = (path: string) =>
	invoke<void>("reveal_in_explorer", { path });

// ---- Issues / PRs ----
export const listIssues = (owner: string, repo: string) =>
	invoke<IssueInfo[]>("list_issues", { owner, repo });
export const listPrs = (owner: string, repo: string) =>
	invoke<PrInfo[]>("list_prs", { owner, repo });
export const spawnIssueWorktree = (
	projectId: string,
	number: number,
	title: string,
) => invoke<CheckoutInfo>("spawn_issue_worktree", { projectId, number, title });
export const spawnPrWorktree = (projectId: string, number: number) =>
	invoke<CheckoutInfo>("spawn_pr_worktree", { projectId, number });
export const createPr = (
	projectId: string,
	checkoutPath: string,
	title: string,
	body: string,
) => invoke<string>("create_pr", { projectId, checkoutPath, title, body });

// ---- Actions ----
export const listWorkflows = (owner: string, repo: string) =>
	invoke<WorkflowInfo[]>("list_workflows", { owner, repo });
export const workflowDetails = (
	owner: string,
	repo: string,
	workflowId: number,
) => invoke<WorkflowDetails>("workflow_details", { owner, repo, workflowId });
export const listRuns = (owner: string, repo: string, workflowId: number) =>
	invoke<RunInfo[]>("list_runs", { owner, repo, workflowId });
export const rerunRun = (owner: string, repo: string, runId: number) =>
	invoke<void>("rerun_run", { owner, repo, runId });
export const cancelRun = (owner: string, repo: string, runId: number) =>
	invoke<void>("cancel_run", { owner, repo, runId });
export const dispatchWorkflow = (
	owner: string,
	repo: string,
	workflowId: number,
	ref: string,
	inputs: Record<string, string> = {},
) =>
	invoke<void>("dispatch_workflow", { owner, repo, workflowId, ref, inputs });
export const latestRunForBranch = (
	owner: string,
	repo: string,
	branch: string,
) => invoke<RunInfo | null>("latest_run_for_branch", { owner, repo, branch });
