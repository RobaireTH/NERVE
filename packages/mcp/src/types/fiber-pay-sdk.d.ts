// Type stubs for @fiber-pay/sdk until the package is published.
// Once @fiber-pay/sdk is installed, its bundled types take precedence.

declare module '@fiber-pay/sdk' {
	export class FiberRpcClient {
		constructor(url: string);
		call<T = unknown>(method: string, params: Record<string, unknown>): Promise<T>;
	}

	export class FiberRpcError extends Error {
		code: number;
		data?: unknown;
	}

	export function ckbToShannons(ckb: number): bigint;
	export function randomBytes32(): string;
	export function nodeIdToPeerId(nodeId: string): string;
}
