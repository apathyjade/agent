// Barrel file — re-exports all Tauri IPC API functions by domain.
// Existing imports (`import * as api from '../api/tauri'`) continue to work.
// New consumers may import directly from the domain file for clarity:
//   import { createSession } from '../api/session';
//   import { listProviders } from '../api/config';

export * from './session';
export * from './config';
export * from './management';
