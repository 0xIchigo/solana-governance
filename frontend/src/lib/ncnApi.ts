/**
 * Shared client for the NCN verifier API (snapshot meta, voter summaries, merkle proofs).
 *
 * `ncn-governance.solana.com` is a router that 302-redirects each request to a randomly
 * chosen verifier operator's own domain, so every call spans two origins and two chances
 * to fail. When either hop fails, the failure often arrives without CORS headers (a
 * Cloudflare 5xx error page carries none), which the browser reports as an opaque
 * `TypeError` rather than a readable status. This module gives those failures a name, a
 * host, and a bounded deadline.
 */

/** Default NCN API base URL. Users can override it via the settings modal (see NcnApiContext). */
export const DEFAULT_NCN_API_URL = "https://ncn-governance.solana.com";

/** The router stalls for ~20s when unhealthy; fail sooner and let React Query retry. */
const DEFAULT_TIMEOUT_MS = 10_000;

/** The API answered, but with a non-2xx status. */
export class NcnApiHttpError extends Error {
  readonly status: number;

  constructor(label: string, status: number, statusText: string) {
    // These endpoints are served over HTTP/2, which has no reason phrase, so statusText is
    // almost always empty. Always include the numeric status.
    super(
      `Failed to get ${label}: ${statusText ? `${status} ${statusText}` : status}`
    );
    this.name = "NcnApiHttpError";
    this.status = status;
  }
}

/**
 * No readable response ever arrived: DNS/TLS failure, timeout, refused connection, or a
 * cross-origin response that failed the CORS check.
 */
export class NcnApiNetworkError extends Error {
  readonly url: string;

  constructor(message: string, url: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = "NcnApiNetworkError";
    this.url = url;
  }
}

const hostOf = (url: string): string => {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
};

/**
 * Whether an error is a network-level fetch failure rather than an application error.
 *
 * Browsers reject `fetch` with a bare `TypeError` when the request never completed, and the
 * message is engine specific: "Load failed" (Safari/WebKit), "Failed to fetch" (Chrome),
 * "NetworkError when attempting to fetch resource" (Firefox), "fetch failed" (Node/undici).
 */
export const isNetworkFailure = (error: unknown): boolean => {
  if (error instanceof NcnApiNetworkError) return true;

  return (
    error instanceof TypeError &&
    /load failed|failed to fetch|fetch failed|networkerror|network request failed/i.test(
      error.message
    )
  );
};

interface FetchNcnJsonOptions {
  /**
   * Pass React Query's `signal` through so unmounting or invalidating the query aborts the
   * request instead of leaving it in flight to fail later.
   */
  signal?: AbortSignal;
  timeoutMs?: number;
  /** Human-readable name of the resource, used in error messages. */
  label: string;
}

export async function fetchNcnJson<T>(
  url: string,
  { signal, timeoutMs = DEFAULT_TIMEOUT_MS, label }: FetchNcnJsonOptions
): Promise<T> {
  // Composed by hand rather than with AbortSignal.any/AbortSignal.timeout, which need
  // Safari 17.4+ — and Safari users are the ones hitting these failures.
  const controller = new AbortController();
  let timedOut = false;

  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, timeoutMs);

  const abortFromCaller = () => controller.abort();
  signal?.addEventListener("abort", abortFromCaller);
  // "abort" does not fire for a signal that was already aborted before we subscribed.
  if (signal?.aborted) controller.abort();

  try {
    const response = await fetch(url, { signal: controller.signal });

    if (!response.ok) {
      throw new NcnApiHttpError(label, response.status, response.statusText);
    }

    return (await response.json()) as T;
  } catch (error) {
    // The caller cancelled us (unmount, invalidation). Rethrow untouched so React Query
    // records a cancellation rather than a failure — cancellations never reach
    // QueryCache.onError, so they are never reported to Sentry.
    if (signal?.aborted) throw error;

    if (timedOut) {
      throw new NcnApiNetworkError(
        `Timed out after ${timeoutMs}ms getting ${label} from ${hostOf(url)}`,
        url,
        { cause: error }
      );
    }

    if (isNetworkFailure(error)) {
      throw new NcnApiNetworkError(
        `NCN API unreachable at ${hostOf(url)} while getting ${label} (network or CORS failure)`,
        url,
        { cause: error }
      );
    }

    throw error;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener("abort", abortFromCaller);
  }
}
