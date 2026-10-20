import { useCallback, useEffect, useRef, useState } from "react";

import { formatError } from "../format";
import type {
  UniswapComparison,
  UniswapPreparedStep,
  UniswapQuote,
  UniswapState,
  WalletSession,
} from "../types";
import { walletApi } from "../walletApi";
import type { CctpRunner } from "./BridgeView";
import { canSubmitFromQuote, quoteProvider, selectComparedQuote } from "./uniswapFlow";

const EMPTY: UniswapState = { networks: [], operations: [] };

/** Owns wallet calls and timers; the screen receives data and action callbacks. */
export function useUniswap({
  active,
  walletKey,
  run,
  onSession,
}: {
  active: boolean;
  walletKey: string;
  run: CctpRunner;
  onSession: (session: WalletSession) => void;
}) {
  const [data, setData] = useState<UniswapState>(EMPTY);
  const [quote, setQuote] = useState<UniswapQuote | null>(null);
  const [quotes, setQuotes] = useState<UniswapQuote[]>([]);
  const [unavailable, setUnavailable] = useState<UniswapComparison["unavailable"]>([]);
  const preferredProvider = useRef<string | null>(null);
  const [review, setReview] = useState<UniswapPreparedStep | null>(null);
  const [error, setError] = useState<string | null>(null);
  const epoch = useRef(0);
  const quoteRequest = useRef(0);
  const live = useRef(false);
  const currentKey = useRef(walletKey);
  if (currentKey.current !== walletKey) {
    currentKey.current = walletKey;
    epoch.current += 1;
  }

  const perform = useCallback(
    async <T>(
      action: (isCurrent: () => boolean) => Promise<T>,
      silent = false,
    ): Promise<T | null> => {
      const started = epoch.current;
      const isCurrent = () => live.current && epoch.current === started;
      return run(
        async () => {
          if (!isCurrent()) return null;
          setError(null);
          try {
            const result = await action(isCurrent);
            return isCurrent() ? result : null;
          } catch (failure) {
            if (isCurrent()) setError(formatError(failure));
            throw failure;
          }
        },
        { silent },
      );
    },
    [run],
  );

  const load = useCallback(
    () =>
      perform(async (isCurrent) => {
        const next = await walletApi.getUniswapState();
        if (isCurrent()) setData(next);
        return next;
      }),
    [perform],
  );

  useEffect(() => {
    live.current = active;
    epoch.current += 1;
    setData(EMPTY);
    setQuote(null);
    setQuotes([]);
    setUnavailable([]);
    preferredProvider.current = null;
    setReview(null);
    setError(null);
    if (active) void load();
    return () => {
      live.current = false;
      epoch.current += 1;
    };
  }, [active, load, walletKey]);

  const clearQuote = useCallback(() => {
    quoteRequest.current += 1;
    setQuote(null);
    setQuotes([]);
    setUnavailable([]);
    preferredProvider.current = null;
    setError(null);
  }, []);

  const requestQuote = useCallback(
    (args: Parameters<typeof walletApi.quoteUniswapSwap>[0]) => {
      const serial = ++quoteRequest.current;
      return perform(async (isCurrent) => {
        try {
          const next = await walletApi.quoteUniswapSwap(args);
          if (!isCurrent() || quoteRequest.current !== serial) return null;
          const selected = selectComparedQuote(next.quotes, preferredProvider.current);
          setQuotes(next.quotes);
          setUnavailable(next.unavailable);
          setQuote(selected);
          return selected;
        } catch (failure) {
          if (!isCurrent() || quoteRequest.current !== serial) return null;
          setQuote(null);
          setQuotes([]);
          setUnavailable([]);
          preferredProvider.current = null;
          throw failure;
        }
      }, true);
    },
    [perform],
  );

  const selectQuote = useCallback(
    (id: string) => {
      const selected = quotes.find((candidate) => candidate.id === id);
      if (!selected) return;
      preferredProvider.current = quoteProvider(selected);
      setQuote(selected);
    },
    [quotes],
  );

  const start = useCallback(() => {
    const approvedQuote = quote;
    if (!approvedQuote) return Promise.resolve(null);
    return perform(async (isCurrent) => {
      try {
        const created = await walletApi.createUniswapSwap({ quoteId: approvedQuote.id });
        if (!isCurrent()) return null;
        onSession(created.session);
        setQuote(null);
        setQuotes([]);
        setUnavailable([]);
        preferredProvider.current = null;
        const prepared = await walletApi.prepareUniswapSwap({ operationId: created.operation_id });
        if (!isCurrent()) return null;
        setReview(prepared);
        if (!canSubmitFromQuote(approvedQuote, prepared)) return true;
        const session = await walletApi.submitUniswapSwap({
          operationId: created.operation_id,
          expectedHash: prepared.tx_hash,
        });
        if (isCurrent()) {
          onSession(session);
          setReview(null);
        }
        return true;
      } finally {
        if (isCurrent()) {
          const next = await walletApi.getUniswapState();
          if (isCurrent()) setData(next);
        }
      }
    });
  }, [onSession, perform, quote]);

  const prepare = useCallback(
    (operationId: string) =>
      perform(async (isCurrent) => {
        const prepared = await walletApi.prepareUniswapSwap({ operationId });
        if (isCurrent()) setReview(prepared);
        const next = await walletApi.getUniswapState();
        if (isCurrent()) setData(next);
        return prepared;
      }),
    [perform],
  );

  const submit = useCallback(
    (operationId: string, expectedHash: string) =>
      perform(async (isCurrent) => {
        try {
          const session = await walletApi.submitUniswapSwap({ operationId, expectedHash });
          if (isCurrent()) {
            onSession(session);
            setReview(null);
          }
          return true;
        } finally {
          if (isCurrent()) {
            const next = await walletApi.getUniswapState();
            if (isCurrent()) setData(next);
          }
        }
      }),
    [onSession, perform],
  );

  const sync = useCallback(
    (operationId: string) =>
      perform(async (isCurrent) => {
        const session = await walletApi.syncUniswapSwap({ operationId });
        if (isCurrent()) onSession(session);
        const next = await walletApi.getUniswapState();
        if (isCurrent()) setData(next);
        return next;
      }),
    [onSession, perform],
  );

  useEffect(() => {
    if (!active) return;
    const pending = data.operations.filter((operation) => operation.needs_sync);
    if (pending.length === 0) return;
    const timer = setInterval(() => {
      // The synchronous App operation gate skips a tick when another action owns it.
      void sync(pending[0].id);
    }, 3000);
    return () => clearInterval(timer);
  }, [active, data.operations, sync]);

  const cancel = useCallback(
    (operationId: string) =>
      perform(async (isCurrent) => {
        const session = await walletApi.cancelUniswapSwap({ operationId });
        if (isCurrent()) {
          onSession(session);
          setReview(null);
        }
        const next = await walletApi.getUniswapState();
        if (isCurrent()) setData(next);
        return true;
      }),
    [onSession, perform],
  );

  const stop = useCallback(() => {
    epoch.current += 1;
    quoteRequest.current += 1;
    setQuote(null);
    setQuotes([]);
    setUnavailable([]);
    preferredProvider.current = null;
    setReview(null);
  }, []);

  return {
    data,
    quote,
    quotes,
    unavailable,
    selectQuote,
    review,
    error,
    clearQuote,
    requestQuote,
    start,
    prepare,
    submit,
    sync,
    cancel,
    load,
    stop,
  };
}

export type UniswapController = ReturnType<typeof useUniswap>;
