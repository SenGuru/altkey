// react-query hooks wrapping the generated @hey-api SDK.
// All SDK functions return { data, error, response } — we unwrap .data here.
// Import api.ts side-effect to ensure the client is configured before any call.
import '../lib/api';

// ─── Local interfaces for list endpoints whose OpenAPI spec returns anonymous arrays ─
// ListAgentsResponse and ListKeysResponse are typed `unknown` in types.gen.ts
// (the spec uses anonymous array schemas). These interfaces match the actual
// JSON shape the control-plane returns.
export interface AgentView {
  id: string;
  name: string;
  token_prefix: string;
  handle_id: string;
  status: string;
}

export interface KeyView {
  id: string;
  name: string;
  key_prefix: string;
  created_at: string;
  revoked_at: string | null;
}

import {
  useQuery,
  useMutation,
  useQueryClient,
  type UseQueryResult,
} from '@tanstack/react-query';
import {
  me,
  subscription,
  listHandles,
  createHandle,
  deleteHandle,
  listAgents,
  createAgent,
  deleteAgent,
  listKeys,
  createKey,
  deleteKey,
  usageSummary,
  listAdapters,
  checkout,
  portal,
  request as requestMagicLink,
  logout,
} from '../client/services.gen';
import type {
  Me,
  SubscriptionView,
  HandleView,
  CreatedAgent,
  CreatedKey,
  RollupView,
  AdapterView,
  UrlResponse,
  CreateAgentRequest,
  CreateKeyRequest,
  CheckoutRequest,
  ClaimHandleRequest,
} from '../client/types.gen';

// ─── Query key constants ──────────────────────────────────────────────────────
export const QK = {
  me: ['me'] as const,
  subscription: ['subscription'] as const,
  handles: ['handles'] as const,
  agents: ['agents'] as const,
  keys: ['keys'] as const,
  usageSummary: ['usageSummary'] as const,
  adapters: ['adapters'] as const,
};

// ─── Queries ─────────────────────────────────────────────────────────────────

/**
 * Returns the current user, or null on 401/error (used by the auth guard —
 * does NOT throw so the guard can redirect rather than surface an error).
 */
export function useMe(): UseQueryResult<Me | null> {
  return useQuery({
    queryKey: QK.me,
    queryFn: async () => {
      const { data, error } = await me();
      if (error || !data) return null;
      return data as Me;
    },
    retry: false,
  });
}

export function useSubscription(): UseQueryResult<SubscriptionView | null> {
  return useQuery({
    queryKey: QK.subscription,
    queryFn: async () => {
      const { data, error } = await subscription();
      if (error || !data) return null;
      return data as SubscriptionView;
    },
  });
}

export function useHandles(): UseQueryResult<HandleView[]> {
  return useQuery({
    queryKey: QK.handles,
    queryFn: async () => {
      const { data, error } = await listHandles();
      if (error || !data) return [];
      return data as HandleView[];
    },
  });
}

export function useAgents(): UseQueryResult<AgentView[]> {
  return useQuery({
    queryKey: QK.agents,
    queryFn: async () => {
      const { data, error } = await listAgents();
      if (error || !data) return [];
      return data as AgentView[];
    },
  });
}

export function useKeys(): UseQueryResult<KeyView[]> {
  return useQuery({
    queryKey: QK.keys,
    queryFn: async () => {
      const { data, error } = await listKeys();
      if (error || !data) return [];
      return data as KeyView[];
    },
  });
}

export function useUsageSummary(): UseQueryResult<RollupView[]> {
  return useQuery({
    queryKey: QK.usageSummary,
    queryFn: async () => {
      const { data, error } = await usageSummary();
      if (error || !data) return [];
      return data as RollupView[];
    },
  });
}

export function useAdapters(): UseQueryResult<AdapterView[]> {
  return useQuery({
    queryKey: QK.adapters,
    queryFn: async () => {
      const { data, error } = await listAdapters();
      if (error || !data) return [];
      return data as AdapterView[];
    },
  });
}

// ─── Mutations ────────────────────────────────────────────────────────────────

export function useCreateHandle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (body: ClaimHandleRequest): Promise<HandleView> => {
      const { data, error } = await createHandle({ body });
      if (error || !data) throw new Error('createHandle failed');
      return data as HandleView;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: QK.handles });
    },
  });
}

export function useDeleteHandle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (id: string): Promise<void> => {
      const { error } = await deleteHandle({ path: { id } });
      if (error) throw new Error('deleteHandle failed');
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: QK.handles });
    },
  });
}

export function useCreateAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (body: CreateAgentRequest): Promise<CreatedAgent> => {
      const { data, error } = await createAgent({ body });
      if (error || !data) throw new Error('createAgent failed');
      return data as CreatedAgent;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: QK.agents });
    },
  });
}

export function useDeleteAgent() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (id: string): Promise<void> => {
      const { error } = await deleteAgent({ path: { id } });
      if (error) throw new Error('deleteAgent failed');
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: QK.agents });
    },
  });
}

export function useCreateKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (body: CreateKeyRequest): Promise<CreatedKey> => {
      const { data, error } = await createKey({ body });
      if (error || !data) throw new Error('createKey failed');
      return data as CreatedKey;
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: QK.keys });
    },
  });
}

export function useDeleteKey() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (id: string): Promise<void> => {
      const { error } = await deleteKey({ path: { id } });
      if (error) throw new Error('deleteKey failed');
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: QK.keys });
    },
  });
}

export function useCheckout() {
  return useMutation({
    mutationFn: async (body: CheckoutRequest): Promise<UrlResponse> => {
      const { data, error } = await checkout({ body });
      if (error || !data) throw new Error('checkout failed');
      return data as UrlResponse;
    },
  });
}

export function usePortal() {
  return useMutation({
    mutationFn: async (): Promise<UrlResponse> => {
      const { data, error } = await portal();
      if (error || !data) throw new Error('portal failed');
      return data as UrlResponse;
    },
  });
}

export function useRequestMagicLink() {
  return useMutation({
    mutationFn: async (email: string): Promise<void> => {
      const { error } = await requestMagicLink({ body: { email } });
      if (error) throw new Error('magic-link request failed');
    },
  });
}

export function useLogout() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async (): Promise<void> => {
      const { error } = await logout();
      if (error) throw new Error('logout failed');
    },
    onSuccess: () => {
      // Clear all cached data so there's no stale auth state after logout.
      qc.clear();
    },
  });
}
