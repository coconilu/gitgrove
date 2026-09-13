import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

export function releaseTarget(eventName, event, sha, ref, preparedSha) {
	const branch = event.repository.default_branch;
	let target = sha;
	if (eventName === "workflow_dispatch") {
		if (ref !== "refs/heads/" + branch)
			throw new Error("请从默认分支运行发布流程");
		if (event.inputs.bump !== "none") {
			if (!["patch", "minor", "major"].includes(event.inputs.bump))
				throw new Error("未知的版本增量");
			if (!preparedSha) return null;
			target = preparedSha;
		}
	} else {
		throw new Error("不支持的发布事件: " + eventName);
	}
	if (!/^[a-f0-9]{40}$/.test(target)) throw new Error("无效的发布提交");
	return { sha: target };
}

export function releaseTag(pkgVersion, tauriVersion) {
	if (!/^\d+\.\d+\.\d+$/.test(pkgVersion) || pkgVersion !== tauriVersion)
		throw new Error("package.json 与 tauri.conf.json 必须使用相同的有效版本号");
	return "v" + pkgVersion;
}

export function hasRequiredCheck(checks) {
	const check = checks
		.filter((c) => c.name === "check" && c.app?.id === 15368)
		.sort((a, b) => b.id - a.id)[0];
	return check?.status === "completed" && check.conclusion === "success";
}

async function github(endpoint, missing = false) {
	const response = await fetch(
		"https://api.github.com/repos/" + process.env.GITHUB_REPOSITORY + endpoint,
		{
			headers: {
				Authorization: "Bearer " + process.env.GH_TOKEN,
				Accept: "application/vnd.github+json",
			},
		},
	);
	if (response.status === 404 && missing) return null;
	if (!response.ok)
		throw new Error("GitHub " + endpoint + ": HTTP " + response.status);
	return response.json();
}

async function main() {
	const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
	const target = releaseTarget(
		process.env.GITHUB_EVENT_NAME,
		event,
		process.env.GITHUB_SHA,
		process.env.GITHUB_REF,
		process.env.PREPARED_SHA,
	);
	const output = (values) => {
		for (const [key, value] of Object.entries(values))
			appendFileSync(process.env.GITHUB_OUTPUT, key + "=" + value + "\n");
	};
	if (!target) return output({ publish: false });
	const branch = event.repository.default_branch;
	execFileSync("git", ["fetch", "origin", branch], { stdio: "inherit" });
	execFileSync("git", [
		"merge-base",
		"--is-ancestor",
		target.sha,
		"origin/" + branch,
	]);
	const readAt = (file) =>
		JSON.parse(
			execFileSync("git", ["show", target.sha + ":" + file], {
				encoding: "utf8",
			}),
		);
	const tag = releaseTag(
		readAt("app/package.json").version,
		readAt("app/src-tauri/tauri.conf.json").version,
	);
	const release = await github("/releases/tags/" + tag, true);
	if (release && !release.draft) {
		console.log(tag + " 已发布，跳过；不会覆盖现有版本。");
		return output({ publish: false });
	}
	// 验证同一个提交的最新必需检查；不接受旧提交或同名第三方状态。
	const checks = await github(
		"/commits/" + target.sha + "/check-runs?filter=latest&per_page=100",
	);
	if (!hasRequiredCheck(checks.check_runs))
		throw new Error(
			"提交 " + target.sha + " 尚未通过 check，请先在该提交运行 CI",
		);
	const tagRef = await github("/git/ref/tags/" + tag, true);
	if (tagRef) {
		let object = tagRef.object;
		while (object.type === "tag")
			object = (await github("/git/tags/" + object.sha)).object;
		if (object.type !== "commit" || object.sha !== target.sha)
			throw new Error(tag + " 已指向其他提交，不能覆盖");
	}
	output({
		publish: true,
		sha: target.sha,
		tag,
	});
	console.log("发布固定提交: " + target.sha + " -> " + tag);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href)
	main().catch((error) => {
		console.error(error.message);
		process.exitCode = 1;
	});
