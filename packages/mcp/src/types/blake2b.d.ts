// Type stubs for the blake2b npm package.

declare module 'blake2b' {
	interface Blake2bHash {
		update(input: Uint8Array | Buffer): Blake2bHash;
		digest(): Uint8Array;
		digest(out: Uint8Array): Uint8Array;
		digest(encoding: 'hex'): string;
	}

	function blake2b(
		outlen: number,
		key?: Uint8Array | Buffer,
		salt?: Uint8Array | Buffer,
		personal?: Uint8Array | Buffer,
		noAssert?: boolean,
	): Blake2bHash;

	export default blake2b;
}
