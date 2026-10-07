import { QueryClient } from '@tanstack/react-query';

export const queryClient = new QueryClient({ defaultOptions: {
  queries: { staleTime: 0, gcTime: 0, retry: false, refetchOnWindowFocus: true, refetchOnReconnect: true },
  mutations: { retry: false },
} });
export const meQueryKey = ['identity', 'me'] as const;

export async function invalidateIdentity(): Promise<void> {
  await queryClient.invalidateQueries({ queryKey: ['identity'] });
}
