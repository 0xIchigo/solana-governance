"use client";

import { QueryCache, QueryClient } from "@tanstack/react-query";
import { PersistQueryClientProvider } from "@tanstack/react-query-persist-client";
import { createAsyncStoragePersister } from "@tanstack/query-async-storage-persister";
import { GET_GOVERNANCE_CONFIG, GET_PROPOSAL_DOCUMENT } from "@/helpers";
import { isNetworkFailure } from "@/lib/ncnApi";
import AppWalletProvider from "../components/AppWalletProvider";
import { EndpointProvider } from "../contexts/EndpointContext";
import { GovernanceConfigProvider } from "../contexts/GovernanceConfigContext";
import { NcnApiProvider } from "../contexts/NcnApiContext";
import { captureException } from "@sentry/nextjs";

const GOVERNANCE_CONFIG_PERSIST_MAX_AGE_MS = 60 * 60 * 1000; // 1 hour (aligns with useGovernanceConfig stale time)

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: 1000 * 10 },
  },
  queryCache: new QueryCache({
    // Fires once per query after its retries are exhausted, never for cancellations.
    onError: (error, query) => {
      console.error("Query error:", error);

      const queryKey = String(query.queryKey[0]);
      // Proposal documents are fetched from GitHub, which is outside our control and rate
      // limited per IP. A failure there degrades to showing just the raw link, so it is not
      // worth an alert.
      if (queryKey === GET_PROPOSAL_DOCUMENT) return;

      // Upstream being unreachable is an infrastructure event, not a bug in the app. Report
      // it as a warning and collapse it to one issue per query, otherwise a single upstream
      // outage buries everything else in identical anonymous TypeErrors.
      if (isNetworkFailure(error)) {
        captureException(error, {
          level: "warning",
          tags: { query_key: queryKey, failure_kind: "network" },
          fingerprint: ["network-failure", queryKey],
        });
        return;
      }

      captureException(error, { tags: { query_key: queryKey } });
    },
  }),
});

const governanceConfigPersister = createAsyncStoragePersister({
  storage: typeof window === "undefined" ? undefined : window.localStorage,
  key: "REACT_QUERY_GOVERNANCE_CONFIG",
});

export default function Providers({ children }: { children: React.ReactNode }) {
  return (
    <PersistQueryClientProvider
      client={queryClient}
      persistOptions={{
        persister: governanceConfigPersister,
        maxAge: GOVERNANCE_CONFIG_PERSIST_MAX_AGE_MS,
        dehydrateOptions: {
          shouldDehydrateQuery: (query) =>
            query.queryKey[0] === GET_GOVERNANCE_CONFIG,
        },
      }}
    >
      <EndpointProvider>
        <NcnApiProvider>
          <GovernanceConfigProvider>
            <AppWalletProvider>{children}</AppWalletProvider>
          </GovernanceConfigProvider>
        </NcnApiProvider>
      </EndpointProvider>
    </PersistQueryClientProvider>
  );
}
