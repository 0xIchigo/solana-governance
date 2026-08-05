import {
  fetchNcnJson,
  isNetworkFailure,
  NcnApiHttpError,
  NcnApiNetworkError,
} from "../ncnApi";

const URL_UNDER_TEST = "https://ncn-governance.solana.com/meta?network=mainnet";

describe("fetchNcnJson", () => {
  const originalFetch = global.fetch;

  afterEach(() => {
    global.fetch = originalFetch;
    jest.useRealTimers();
    jest.restoreAllMocks();
  });

  it("returns the parsed body on success", async () => {
    const meta = { network: "mainnet", slot: 422497000 };
    global.fetch = jest.fn().mockResolvedValue({
      ok: true,
      status: 200,
      json: async () => meta,
    }) as unknown as typeof fetch;

    await expect(
      fetchNcnJson(URL_UNDER_TEST, { label: "snapshot meta info" })
    ).resolves.toEqual(meta);
  });

  it("throws NcnApiHttpError carrying the status on a non-ok response", async () => {
    // HTTP/2 has no reason phrase, so statusText is empty in practice — the numeric status
    // is the only thing that identifies the failure.
    global.fetch = jest.fn().mockResolvedValue({
      ok: false,
      status: 503,
      statusText: "",
      json: async () => ({}),
    }) as unknown as typeof fetch;

    const error = await fetchNcnJson(URL_UNDER_TEST, {
      label: "snapshot meta info",
    }).catch((e: unknown) => e);

    expect(error).toBeInstanceOf(NcnApiHttpError);
    expect((error as NcnApiHttpError).status).toBe(503);
    expect((error as Error).message).toContain("503");
  });

  it("wraps a network-level TypeError, naming the host", async () => {
    // What Safari actually throws when a cross-origin response fails the CORS check.
    global.fetch = jest
      .fn()
      .mockRejectedValue(new TypeError("Load failed")) as unknown as typeof fetch;

    const error = await fetchNcnJson(URL_UNDER_TEST, {
      label: "snapshot meta info",
    }).catch((e: unknown) => e);

    expect(error).toBeInstanceOf(NcnApiNetworkError);
    expect((error as Error).message).toContain("ncn-governance.solana.com");
    expect((error as Error).message).toContain("snapshot meta info");
    expect((error as NcnApiNetworkError).cause).toBeInstanceOf(TypeError);
  });

  it("throws NcnApiNetworkError when the request exceeds timeoutMs", async () => {
    // Never settles on its own; only the internal timeout can abort it.
    global.fetch = jest.fn(
      (_url: unknown, init?: { signal?: AbortSignal }) =>
        new Promise((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () =>
            reject(new DOMException("Aborted", "AbortError"))
          );
        })
    ) as unknown as typeof fetch;

    const pending = fetchNcnJson(URL_UNDER_TEST, {
      label: "snapshot meta info",
      timeoutMs: 50,
    }).catch((e: unknown) => e);

    const error = await pending;

    expect(error).toBeInstanceOf(NcnApiNetworkError);
    expect((error as Error).message).toContain("Timed out after 50ms");
  });

  it("does not start work when the caller's signal is already aborted", async () => {
    global.fetch = jest.fn(
      (_url: unknown, init?: { signal?: AbortSignal }) =>
        new Promise((resolve, reject) => {
          if (init?.signal?.aborted) {
            reject(new DOMException("Aborted", "AbortError"));
            return;
          }
          resolve({ ok: true, status: 200, json: async () => ({}) });
        })
    ) as unknown as typeof fetch;

    const error = await fetchNcnJson(URL_UNDER_TEST, {
      label: "snapshot meta info",
      signal: AbortSignal.abort(),
    }).catch((e: unknown) => e);

    expect((error as Error).name).toBe("AbortError");
  });

  it("rethrows the caller's AbortError untouched so React Query sees a cancellation", async () => {
    global.fetch = jest.fn(
      (_url: unknown, init?: { signal?: AbortSignal }) =>
        new Promise((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () =>
            reject(new DOMException("Aborted", "AbortError"))
          );
        })
    ) as unknown as typeof fetch;

    const controller = new AbortController();
    const pending = fetchNcnJson(URL_UNDER_TEST, {
      label: "snapshot meta info",
      signal: controller.signal,
    }).catch((e: unknown) => e);

    controller.abort();
    const error = await pending;

    expect(error).not.toBeInstanceOf(NcnApiNetworkError);
    expect((error as Error).name).toBe("AbortError");
  });
});

describe("isNetworkFailure", () => {
  it.each([
    ["Load failed", true], // Safari / WebKit
    ["Failed to fetch", true], // Chrome
    ["NetworkError when attempting to fetch resource.", true], // Firefox
    ["fetch failed", true], // Node / undici
    ["Cannot read properties of undefined", false],
  ])("classifies TypeError(%s) as %s", (message, expected) => {
    expect(isNetworkFailure(new TypeError(message))).toBe(expected);
  });

  it("classifies NcnApiNetworkError as a network failure", () => {
    expect(
      isNetworkFailure(new NcnApiNetworkError("unreachable", URL_UNDER_TEST))
    ).toBe(true);
  });

  it("does not classify an HTTP error as a network failure", () => {
    expect(isNetworkFailure(new NcnApiHttpError("meta", 503, ""))).toBe(false);
  });
});
