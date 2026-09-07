import { installLocked, type UpdateState } from "../updates";
import { updates, useUpdates } from "../useUpdates";

function InstallFeedback({ state }: { state: UpdateState }) {
	const { installStatus, downloaded, total } = state;
	const percent = total
		? Math.min(100, Math.floor((downloaded / total) * 100))
		: null;
	if (installStatus === "idle") return null;
	return (
		<div className="update-feedback" role="status">
			{installStatus === "downloading" && (
				<>
					<div>正在下载更新…{percent !== null ? ` ${percent}%` : ""}</div>
					<progress
						aria-label="更新下载进度"
						max={100}
						value={percent ?? undefined}
					/>
				</>
			)}
			{installStatus === "installing" && <div>正在安装更新，请稍候…</div>}
			{installStatus === "installed" && (
				<div>更新已安装，请关闭并重新启动 GitGrove 以使用新版本。</div>
			)}
			{installStatus === "error" && (
				<div className="inline-error">更新失败，请重试。</div>
			)}
		</div>
	);
}

function InstallButton({ state }: { state: UpdateState }) {
	return (
		<button
			type="button"
			className="btn primary"
			disabled={
				state.checkStatus === "checking" || installLocked(state.installStatus)
			}
			onClick={() => void updates.install()}
		>
			{state.installStatus === "error"
				? "重试更新"
				: state.installStatus === "installed"
					? "已安装"
					: installLocked(state.installStatus)
						? "更新中"
						: "立即更新"}
		</button>
	);
}

export function UpdateToast() {
	const state = useUpdates();
	if (
		!state.availableVersion ||
		state.dismissedVersions.includes(state.availableVersion)
	)
		return null;
	return (
		<section className="update-toast" aria-label="版本更新提示">
			<div className="update-toast-text">
				<div>
					发现新版本 <b>v{state.availableVersion}</b>
					{state.currentVersion && <>（当前 v{state.currentVersion}）</>}
				</div>
				<InstallFeedback state={state} />
			</div>
			<InstallButton state={state} />
			<button
				type="button"
				className="btn update-dismiss"
				aria-label="关闭更新提示"
				title="关闭更新提示"
				onClick={updates.dismiss}
			>
				×
			</button>
		</section>
	);
}

export function AboutPanel() {
	const state = useUpdates();
	return (
		<section className="about-panel" aria-labelledby="about-title">
			<div className="about-identity">
				<h4 id="about-title">GitGrove</h4>
				<p>
					当前版本{" "}
					{state.currentVersion
						? `v${state.currentVersion}`
						: state.versionError
							? "读取失败"
							: "读取中…"}
				</p>
				{state.versionError && (
					<button
						type="button"
						className="btn sm"
						onClick={() => void updates.loadVersion()}
					>
						重试读取版本
					</button>
				)}
			</div>
			<div className="about-update">
				<div role="status">
					{state.checkStatus === "idle" && <p>尚未检查更新</p>}
					{state.checkStatus === "checking" && <p>正在检查更新…</p>}
					{state.checkStatus === "checked" && !state.availableVersion && (
						<p>已是最新版本</p>
					)}
					{state.checkStatus === "error" && (
						<p className="inline-error">检查更新失败，请检查网络后重试。</p>
					)}
					{state.availableVersion && (
						<p>
							发现新版本 <strong>v{state.availableVersion}</strong>
						</p>
					)}
				</div>
				<InstallFeedback state={state} />
				<div className="about-update-actions">
					<button
						type="button"
						className="btn"
						disabled={
							state.checkStatus === "checking" ||
							installLocked(state.installStatus)
						}
						onClick={() => void updates.checkForUpdates()}
					>
						{state.checkStatus === "checking"
							? "检查中…"
							: state.checkStatus === "error"
								? "重试检查"
								: "检查更新"}
					</button>
					{state.availableVersion && <InstallButton state={state} />}
				</div>
				<p className="settings-hint">
					关闭更新提示后，仍可在这里更新。安装更新时应用可能关闭或需要重新启动。
				</p>
			</div>
		</section>
	);
}
