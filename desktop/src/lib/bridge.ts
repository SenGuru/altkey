import { invoke } from '@tauri-apps/api/core';

export interface AgentStatus {
  running: boolean;
  tunnel_up: boolean;
  handle: string | null;
  reachable_url: string | null;
}

export const agentStatus = () => invoke<AgentStatus>('agent_status');
export const startTunnel = () => invoke<void>('start_tunnel');
export const stopTunnel = () => invoke<void>('stop_tunnel');
export const startAgent = () => invoke<void>('start_agent');
export const openWeb = (path: string) => invoke<void>('open_web', { path });
