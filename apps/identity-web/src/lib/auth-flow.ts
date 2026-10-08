import { useQuery } from '@tanstack/react-query';
import { useSearchParams } from 'react-router';
import { z } from 'zod';
import { api, ApiError, authorizationTransactionSchema } from './api';

export function useAuthTransaction() {
  const [search] = useSearchParams();
  const values = search.getAll('transaction');
  const requested = values.length === 1 && z.uuid().safeParse(values[0]).success ? values[0] : undefined;
  const query = useQuery({
    queryKey: ['identity', 'authorization', requested],
    enabled: requested !== undefined,
    retry: false,
    queryFn: async ({ signal }) => {
      const value = await api.request(`/oauth/transactions/${requested ?? ''}`, authorizationTransactionSchema, { signal });
      if (value.id !== requested || Date.parse(value.expires_at) <= Date.now()) throw new ApiError(400, 'AUTH_ACTION_INVALID', '授权请求已失效，请从应用重新开始。');
      return value;
    },
  });
  const transaction = query.isSuccess && !query.isFetching && query.data.id === requested ? query.data : undefined;
  const invalid = values.length > 0 && requested === undefined;
  const blocked = invalid || requested !== undefined && transaction === undefined;
  const destination = transaction === undefined ? '/me' : `/oauth/consent/${transaction.id}`;
  function link(path: string) { return requested === undefined ? path : `${path}?transaction=${requested}`; }
  return { transaction, blocked, invalid, query, destination, link };
}

export type ReturnTypeOfAuthTransaction = ReturnType<typeof useAuthTransaction>;
