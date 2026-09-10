// src 内部使用 vite 风格的无扩展名相对导入（例如 store.ts 里的 "./api"），
// node 直接加载时没有扩展名推断，这个 resolve 钩子负责补上 .ts。
export async function resolve(specifier, context, nextResolve) {
	try {
		return await nextResolve(specifier, context);
	} catch (error) {
		const relative = specifier.startsWith("./") || specifier.startsWith("../");
		const notFound =
			error instanceof Error && error.code === "ERR_MODULE_NOT_FOUND";
		if (!relative || !notFound) throw error;
		try {
			return await nextResolve(specifier + ".ts", context);
		} catch {
			throw error;
		}
	}
}
