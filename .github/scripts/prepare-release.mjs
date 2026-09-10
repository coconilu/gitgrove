import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import {
	completeVersionRelease,
	mergeVersionPR,
	runCI,
	runPullRequestCI,
} from "./release-ci.mjs";
import { createGitHubAPI } from "./release-github.mjs";
import { hasRequiredCheck } from "./release-target.mjs";

const VERSION_FILES = [
	"app/package.json",
	"app/src-tauri/tauri.conf.json",
	"CHANGELOG.md",
];
const run = (command, args) =>
	execFileSync(command, args, { encoding: "utf8" }).trim();

export function assertVersionFiles(files, actual, expected) {
	if (files.some((file) => !VERSION_FILES.includes(file)))
		throw new Error("版本分支包含版本文件以外的修改，禁止自动合并");
	for (const file of VERSION_FILES) {
		// A rerun on another day may have a different generated heading date.
		const normalize = (text) =>
			file.endsWith(".json")
				? JSON.stringify(JSON.parse(text))
				: text.replace(/（\d{4}-\d{2}-\d{2}）/, "（DATE）").trimEnd();
		if (normalize(actual[file]) !== normalize(expected[file]))
			throw new Error("版本分支内容与自动生成的内容不一致: " + file);
	}
}

// 快速通道前提：master HEAD 已有绿 CI。与 version job 校验合并提交使用同一个
// hasRequiredCheck 标准（GitHub Actions 产出的最新 check 必须 success）。
export async function masterHasGreenCI(api, sha) {
	const { check_runs: checkRuns } = await api(
		"/commits/" + sha + "/check-runs?filter=latest&per_page=100",
	);
	return hasRequiredCheck(checkRuns ?? []);
}

// 快速通道本地校验：版本 PR 只改版本文件且 master 已有绿 CI 时，
// PR 内的源码与主分支完全一致，两个版本文件可解析且版本一致 + biome check 通过即可合并。
export function assertFastTrackVersionFiles(pkgText, tauriText) {
	let pkg;
	let tauri;
	try {
		pkg = JSON.parse(pkgText);
	} catch (error) {
		throw new Error("app/package.json 不是有效的 JSON: " + error.message);
	}
	try {
		tauri = JSON.parse(tauriText);
	} catch (error) {
		throw new Error(
			"app/src-tauri/tauri.conf.json 不是有效的 JSON: " + error.message,
		);
	}
	if (pkg.version !== tauri.version)
		throw new Error(
			"package.json 与 tauri.conf.json 版本不一致: " +
				pkg.version +
				" != " +
				tauri.version,
		);
}

// 快速通道合并：分支保护若要求版本 PR 自身的 check（如 PR #49 撞过的
// "Required status check ... is expected."），回退等待该 PR 的 CI 完成后再合并。
export function mergeWithPRCIFallback({ merge, waitForPRCI }) {
	return async (...args) => {
		try {
			return await merge(...args);
		} catch (error) {
			if (
				error?.status !== 405 ||
				!/required status check/i.test(error.message ?? "")
			)
				throw error;
			await waitForPRCI();
			return merge(...args);
		}
	};
}

async function main() {
	const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
	const branch = event.repository.default_branch;
	const repo = process.env.GITHUB_REPOSITORY;
	const runId = process.env.GITHUB_RUN_ID;
	if (process.env.GITHUB_REF !== "refs/heads/" + branch)
		throw new Error("请从默认分支运行发布流程");
	if (!/^\d+$/.test(runId ?? "")) throw new Error("缺少有效的发布运行 ID");
	const api = createGitHubAPI(repo, process.env.GH_TOKEN);
	const summary = (message) => {
		console.log(message);
		appendFileSync(process.env.GITHUB_STEP_SUMMARY, message + "\n\n");
	};
	const baseSha = run("git", ["rev-parse", "HEAD"]);
	if (baseSha !== process.env.GITHUB_SHA)
		throw new Error("准备版本的源码不是本次触发的提交");
	// 快速通道判定：master HEAD 已有绿 CI 时，版本 PR 只改版本文件、源码与主分支
	// 完全一致，可跳过完整 CI 等待（见下方 checkPR / merge 的 fastTrack 分支）。
	const fastTrack = await masterHasGreenCI(api, baseSha);
	summary(
		fastTrack
			? "master HEAD 已有绿 CI，版本 PR 走快速通道：本地校验版本文件后按分支保护合并"
			: "master HEAD 尚无绿 CI，版本 PR 等待完整 CI 后合并",
	);
	// Each run owns a branch. Historical release-v* leftovers cannot block a new run.
	const head = "codex/release-" + runId;
	run("node", [".github/scripts/bump-version.mjs", process.env.BUMP]);
	const version = JSON.parse(readFileSync("app/package.json", "utf8")).version;
	const expected = Object.fromEntries(
		VERSION_FILES.map((file) => [file, readFileSync(file, "utf8")]),
	);
	const prs = await api(
		"/pulls?state=all&head=" +
			encodeURIComponent(repo.split("/")[0] + ":" + head),
	);
	let pr = prs[0] ? await api("/pulls/" + prs[0].number) : null;
	const remoteHead = await api("/git/ref/heads/" + head, { missing: true });
	const existingSha =
		remoteHead?.object.sha ?? (pr?.merged ? pr.merge_commit_sha : null);
	if (existingSha) {
		run("git", [
			"fetch",
			"origin",
			"refs/heads/" + (remoteHead ? head : branch),
		]);
		const candidate = existingSha;
		if (remoteHead && run("git", ["rev-parse", "FETCH_HEAD"]) !== candidate)
			throw new Error("版本分支在检查期间发生变化，请重试");
		run("git", ["merge-base", "--is-ancestor", baseSha, candidate]);
		const files = run("git", ["diff", "--name-only", baseSha, candidate])
			.split("\n")
			.filter(Boolean);
		const actual = Object.fromEntries(
			VERSION_FILES.map((file) => [
				file,
				run("git", ["show", candidate + ":" + file]),
			]),
		);
		assertVersionFiles(files, actual, expected);
	} else {
		if (prs.length) throw new Error("版本 PR 的分支已被删除，停止重建");
		run("git", ["config", "user.name", "github-actions[bot]"]);
		run("git", [
			"config",
			"user.email",
			"41898282+github-actions[bot]@users.noreply.github.com",
		]);
		run("git", ["switch", "-c", head]);
		run("git", ["add", ...VERSION_FILES]);
		run("git", ["commit", "-m", "chore: release v" + version]);
		run("git", ["push", "origin", "HEAD:refs/heads/" + head]);
	}
	if (!pr) {
		pr = await api("/pulls", {
			method: "POST",
			body: {
				title: "chore: release v" + version,
				head,
				base: branch,
				body:
					"同步版本号与 CHANGELOG 到 v" +
					version +
					"。\n\n本次手动发布自动处理版本提交：主分支已有绿 CI 时仅做本地快速校验（版本文件可解析 + biome check）后按分支保护合并，若分支保护要求本 PR 的 check 则自动等待其完成；否则等待版本 PR 的完整 CI。合并后再校验合并提交并构建发布。仅包含自动生成的版本文件，无需手动批准 PR 工作流或再次合并。",
			},
		});
	}
	// The list endpoint does not expose the full merged state.
	pr = await api("/pulls/" + pr.number);
	if (
		pr.base.ref !== branch ||
		pr.head.ref !== head ||
		pr.head.repo?.full_name !== repo
	)
		throw new Error("版本 PR 的仓库或分支不匹配，停止发布");
	const verifiedHead =
		remoteHead?.object.sha ?? run("git", ["rev-parse", "HEAD"]);
	if (!pr.merged && pr.head.sha !== verifiedHead)
		throw new Error("版本 PR 的提交发生变化，停止发布");
	summary("版本 PR（自动处理）: " + pr.html_url);
	const sha = await completeVersionRelease(pr, {
		checkPR: fastTrack
			? async () => {
					const pkgText = readFileSync("app/package.json", "utf8");
					const tauriText = readFileSync(
						"app/src-tauri/tauri.conf.json",
						"utf8",
					);
					assertFastTrackVersionFiles(pkgText, tauriText);
					const { devDependencies } = JSON.parse(pkgText);
					const biome = devDependencies?.["@biomejs/biome"];
					if (!biome)
						throw new Error("app/package.json 未声明 @biomejs/biome 版本");
					// 与 CI 的 pnpm lint（biome ci ..）同一标准，固定用仓库声明的版本；
					// src-tauri 当前在 biome 排除列表里会被跳过，取消排除后此处自动全覆盖
					execFileSync(
						"npx",
						[
							"--yes",
							"@biomejs/biome@" + biome,
							"check",
							"app/package.json",
							"app/src-tauri/tauri.conf.json",
						],
						{ stdio: "inherit" },
					);
				}
			: (candidate) => runPullRequestCI(api, candidate),
		check: (ref, commit) => runCI(api, ref, commit),
		assertCurrent: async (ref, commit) => {
			const current = await api("/git/ref/heads/" + ref);
			if (current.object.sha !== commit)
				throw new Error(
					"主分支已发生变化，停止本次发布；可在当前主分支重新运行。",
				);
		},
		merge: fastTrack
			? mergeWithPRCIFallback({
					merge: (number, commit) =>
						mergeVersionPR(api, number, commit, branch, baseSha),
					waitForPRCI: () => {
						summary(
							"分支保护要求版本 PR 自身的 check，回退等待该 CI 完成后合并",
						);
						return runPullRequestCI(api, pr);
					},
				})
			: (number, commit) =>
					mergeVersionPR(api, number, commit, branch, baseSha),
	});
	appendFileSync(process.env.GITHUB_OUTPUT, "sha=" + sha + "\n");
	summary(
		"版本 v" +
			version +
			" 已自动合并并通过 CI，继续构建与发布提交 " +
			sha +
			"。",
	);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href)
	main().catch((error) => {
		console.error(error.message);
		process.exitCode = 1;
	});
