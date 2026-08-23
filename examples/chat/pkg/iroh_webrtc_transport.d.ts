/* tslint:disable */
/* eslint-disable */
/**
 * The `ReadableStreamType` enum.
 *
 * *This API requires the following crate features to be activated: `ReadableStreamType`*
 */

export type ReadableStreamType = "bytes";

/**
 * Live chat handle returned to JS. Keeps the iroh endpoint/connection alive and
 * exposes [`send`](Chat::send).
 */
export class Chat {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Enqueue a text message to the peer (fire-and-forget; delivered in order).
     */
    send(text: string): void;
}

export class IntoUnderlyingByteSource {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    cancel(): void;
    pull(controller: ReadableByteStreamController): Promise<any>;
    start(controller: ReadableByteStreamController): void;
    readonly autoAllocateChunkSize: number;
    readonly type: ReadableStreamType;
}

export class IntoUnderlyingSink {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    abort(reason: any): Promise<any>;
    close(): Promise<any>;
    write(chunk: any): Promise<any>;
}

export class IntoUnderlyingSource {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    cancel(): void;
    pull(controller: ReadableStreamDefaultController): Promise<any>;
}

/**
 * Start a chat over an already-open data channel.
 *
 * * `dc` — the open `RTCDataChannel` (the page did the signaling).
 * * `my_seed` / `remote_seed` — deterministic identity seeds, swapped between
 *   the two sides, so each knows both public keys without discovery.
 * * `is_initiator` — the offerer side dials; the answerer side accepts.
 * * `on_message` — JS callback invoked with each inbound message (one string arg).
 */
export function start_chat(dc: RTCDataChannel, my_seed: number, remote_seed: number, is_initiator: boolean, on_message: Function): Promise<Chat>;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_chat_free: (a: number, b: number) => void;
    readonly chat_send: (a: number, b: number, c: number) => void;
    readonly start_chat: (a: any, b: number, c: number, d: number, e: any) => any;
    readonly __wbg_intounderlyingsink_free: (a: number, b: number) => void;
    readonly intounderlyingsink_abort: (a: number, b: any) => any;
    readonly intounderlyingsink_close: (a: number) => any;
    readonly intounderlyingsink_write: (a: number, b: any) => any;
    readonly __wbg_intounderlyingsource_free: (a: number, b: number) => void;
    readonly intounderlyingsource_cancel: (a: number) => void;
    readonly intounderlyingsource_pull: (a: number, b: any) => any;
    readonly __wbg_intounderlyingbytesource_free: (a: number, b: number) => void;
    readonly intounderlyingbytesource_autoAllocateChunkSize: (a: number) => number;
    readonly intounderlyingbytesource_cancel: (a: number) => void;
    readonly intounderlyingbytesource_pull: (a: number, b: any) => any;
    readonly intounderlyingbytesource_start: (a: number, b: any) => void;
    readonly intounderlyingbytesource_type: (a: number) => number;
    readonly ring_core_0_17_14__bn_mul_mont: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly wasm_bindgen_c6b32ff19ee0a2fc___convert__closures_____invoke___wasm_bindgen_c6b32ff19ee0a2fc___JsValue__core_9b3796e30d99ddb7___result__Result_____wasm_bindgen_c6b32ff19ee0a2fc___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_c6b32ff19ee0a2fc___convert__closures_____invoke___js_sys_a66b50105823d874___Function_fn_wasm_bindgen_c6b32ff19ee0a2fc___JsValue_____wasm_bindgen_c6b32ff19ee0a2fc___sys__Undefined___js_sys_a66b50105823d874___Function_fn_wasm_bindgen_c6b32ff19ee0a2fc___JsValue_____wasm_bindgen_c6b32ff19ee0a2fc___sys__Undefined_______true_: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_c6b32ff19ee0a2fc___convert__closures_____invoke___wasm_bindgen_c6b32ff19ee0a2fc___JsValue______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_c6b32ff19ee0a2fc___convert__closures_____invoke___web_sys_b76b48172b3d9750___features__gen_CloseEvent__CloseEvent______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_c6b32ff19ee0a2fc___convert__closures_____invoke___web_sys_b76b48172b3d9750___features__gen_MessageEvent__MessageEvent______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_c6b32ff19ee0a2fc___convert__closures_____invoke_______true_: (a: number, b: number) => void;
    readonly wasm_bindgen_c6b32ff19ee0a2fc___convert__closures_____invoke_______true__1_: (a: number, b: number) => void;
    readonly wasm_bindgen_c6b32ff19ee0a2fc___convert__closures_____invoke_______true__2_: (a: number, b: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
