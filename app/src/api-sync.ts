import { invoke } from "@tauri-apps/api/core";

// 分支同步（issue #42）：fetch 刷新 remote-tracking 引用，pull 仅快进
// 对应 projects.rs 的 fetch_project / pull_checkout（command 注册见 lib.rs）

/** 拉取后的 ahead/behind，用于即时刷新同步状态 */
export interface AheadBehind {
	ahead: number;
	behind: number;
}

/** git fetch --all --prune：让「已同步」与分支列表反映远端真实状态 */
export const fetchProject = (projectId: string) =>
	invoke<void>("fetch_project", { projectId });

/** git pull --ff-only：分叉不能快进时 reject，错误为 git 原文，直接 toast 展示 */
export const pullCheckout = (path: string) =>
	invoke<AheadBehind>("pull_checkout", { path });
