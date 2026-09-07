import assert from "node:assert/strict";
import test from "node:test";
import { createUpdateController } from "../src/updates.ts";

function deferred() {
	let resolve;
	let reject;
	const promise = new Promise((yes, no) => {
		resolve = yes;
		reject = no;
	});
	return { promise, resolve, reject };
}

function fixture(overrides = {}) {
	let saved = null;
	let target = "0.2.0";
	const calls = { check: 0, install: 0, close: 0, version: 0 };
	const handle = () => ({
		version: target,
		downloadAndInstall: async () => {
			calls.install++;
		},
		close: async () => {
			calls.close++;
		},
	});
	const deps = {
		getVersion: async () => {
			calls.version++;
			return "0.1.2";
		},
		check: async () => {
			calls.check++;
			return handle();
		},
		readDismissed: () => saved,
		writeDismissed: (value) => {
			saved = value;
		},
		...overrides,
	};
	const controller = createUpdateController(deps);
	return {
		controller,
		deps,
		calls,
		setTarget: (value) => {
			target = value;
		},
	};
}

test("关闭按版本持久化，轮询与重新启动不再提示，新版本仍可提示", async () => {
	const f = fixture();
	await f.controller.checkForUpdates();
	f.controller.dismiss();
	await f.controller.checkForUpdates();
	assert.equal(f.controller.store.getState().availableVersion, "0.2.0");
	const restarted = createUpdateController(f.deps);
	await restarted.checkForUpdates();
	assert.deepEqual(restarted.store.getState().dismissedVersions, ["0.2.0"]);
	f.setTarget("0.3.0");
	await restarted.checkForUpdates();
	assert.equal(restarted.store.getState().availableVersion, "0.3.0");
	assert.equal(
		restarted.store.getState().dismissedVersions.includes("0.3.0"),
		false,
	);
	// Dismissing the toast never discards the actionable update handle.
	await restarted.install();
	assert.equal(f.calls.install, 1);
});

test("离线检查失败仍能独立读取运行版本，不伪报已是最新", async () => {
	const f = fixture({
		check: async () => {
			throw new Error("offline");
		},
	});
	await Promise.all([
		f.controller.loadVersion(),
		f.controller.checkForUpdates(),
	]);
	assert.equal(f.controller.store.getState().currentVersion, "0.1.2");
	assert.equal(f.controller.store.getState().checkStatus, "error");
});

test("检查去重，并在检查期间禁止安装旧句柄", async () => {
	const pending = deferred();
	const f = fixture();
	await f.controller.checkForUpdates();
	f.deps.check = () => {
		f.calls.check++;
		return pending.promise;
	};
	const first = f.controller.checkForUpdates();
	await f.controller.checkForUpdates();
	await f.controller.install();
	assert.equal(f.calls.check, 2);
	assert.equal(f.calls.install, 0);
	pending.resolve(null);
	await first;
	assert.equal(f.controller.store.getState().availableVersion, null);
	assert.equal(f.calls.close, 1);
});

test("下载中关闭提示、重复点击与后台检查不取消或重复安装", async () => {
	const pending = deferred();
	let onProgress;
	let installs = 0;
	const f = fixture({
		check: async () => ({
			version: "0.2.0",
			close: async () => {},
			downloadAndInstall: (notify) => {
				installs++;
				onProgress = notify;
				return pending.promise;
			},
		}),
	});
	await Promise.all([
		f.controller.loadVersion(),
		f.controller.checkForUpdates(),
	]);
	const installing = f.controller.install();
	onProgress({ event: "Started", data: { contentLength: 1000 } });
	onProgress({ event: "Progress", data: { chunkLength: 400 } });
	f.controller.dismiss();
	await Promise.all([f.controller.install(), f.controller.checkForUpdates()]);
	assert.equal(installs, 1);
	assert.equal(f.controller.store.getState().downloaded, 400);
	assert.equal(f.controller.store.getState().installStatus, "downloading");
	onProgress({ event: "Finished" });
	assert.equal(f.controller.store.getState().installStatus, "installing");
	pending.resolve();
	await installing;
	assert.equal(f.controller.store.getState().installStatus, "installed");
	assert.equal(f.controller.store.getState().currentVersion, "0.1.2");
	await f.controller.install();
	assert.equal(installs, 1);
});

test("更新失败后释放忙碌锁，可从同一可用版本重试", async () => {
	let installs = 0;
	const f = fixture({
		check: async () => ({
			version: "0.2.0",
			close: async () => {},
			downloadAndInstall: async () => {
				if (++installs === 1) throw new Error("installer failed");
			},
		}),
	});
	await f.controller.checkForUpdates();
	await f.controller.install();
	assert.equal(f.controller.store.getState().installStatus, "error");
	await f.controller.install();
	assert.equal(installs, 2);
	assert.equal(f.controller.store.getState().installStatus, "installed");
});

test("后续检查失败保留已发现的更新，仍可安装；成功的空结果清除旧信息", async () => {
	const f = fixture();
	await f.controller.checkForUpdates();
	f.deps.check = async () => {
		throw new Error("offline");
	};
	await f.controller.checkForUpdates();
	assert.equal(f.controller.store.getState().availableVersion, "0.2.0");
	assert.equal(f.controller.store.getState().checkStatus, "error");
	await f.controller.install();
	assert.equal(f.calls.install, 1);
	const other = fixture();
	await other.controller.checkForUpdates();
	other.deps.check = async () => null;
	await other.controller.checkForUpdates();
	assert.equal(other.controller.store.getState().availableVersion, null);
	assert.equal(other.controller.store.getState().checkStatus, "checked");
	assert.equal(other.calls.close, 1);
});

test("存储损坏或不可写时应用可用，关闭仍在本次会话生效", async () => {
	for (const saved of ["broken json", "{}", '[null,42,"0.1.0"]']) {
		const f = fixture({
			readDismissed: () => saved,
			writeDismissed: () => {
				throw new Error("denied");
			},
		});
		await f.controller.checkForUpdates();
		assert.doesNotThrow(() => f.controller.dismiss());
		assert.equal(
			f.controller.store.getState().dismissedVersions.includes("0.2.0"),
			true,
		);
	}
});

test("运行版本读取去重，失败可重试，成功后不会因打开关于重复读取", async () => {
	const pending = deferred();
	let calls = 0;
	const f = fixture({
		getVersion: () => {
			calls++;
			return pending.promise;
		},
	});
	const first = f.controller.loadVersion();
	await f.controller.loadVersion();
	assert.equal(calls, 1);
	pending.reject(new Error("unavailable"));
	await first;
	assert.equal(f.controller.store.getState().versionError, true);
	f.deps.getVersion = async () => {
		calls++;
		return "0.1.2";
	};
	await f.controller.loadVersion();
	await f.controller.loadVersion();
	assert.equal(calls, 2);
	assert.equal(f.controller.store.getState().currentVersion, "0.1.2");
});

test("更新句柄清理失败不破坏后续检查结果", async () => {
	const f = fixture({
		check: async () => ({
			version: "0.2.0",
			close: async () => {
				throw new Error("closed");
			},
			downloadAndInstall: async () => {},
		}),
	});
	await f.controller.checkForUpdates();
	f.deps.check = async () => null;
	await f.controller.checkForUpdates();
	assert.equal(f.controller.store.getState().checkStatus, "checked");
	assert.equal(f.controller.store.getState().availableVersion, null);
});
