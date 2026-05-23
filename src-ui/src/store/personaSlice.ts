import type { StateCreator } from 'zustand';
import type { PersonaInfo, CreatePersonaParams, UpdatePersonaParams } from '../types';
import * as api from '../api/tauri';

export interface PersonaSlice {
  personas: PersonaInfo[];
  personaLoading: boolean;
  personaError: string | null;

  fetchPersonas: () => Promise<void>;
  createPersona: (params: CreatePersonaParams) => Promise<void>;
  updatePersona: (id: string, params: UpdatePersonaParams) => Promise<void>;
  deletePersona: (id: string) => Promise<void>;
}

export const createPersonaSlice: StateCreator<PersonaSlice, [], [], PersonaSlice> = (set, get) => ({
  personas: [],
  personaLoading: false,
  personaError: null,

  fetchPersonas: async () => {
    set({ personaLoading: true, personaError: null });
    try {
      const personas = await api.listPersonas();
      set({ personas, personaLoading: false });
    } catch (err) {
      set({ personaError: String(err), personaLoading: false });
    }
  },

  createPersona: async (params) => {
    set({ personaLoading: true, personaError: null });
    try {
      await api.createPersona(params);
      await get().fetchPersonas();
    } catch (err) {
      set({ personaError: String(err), personaLoading: false });
      throw err;
    }
  },

  updatePersona: async (id, params) => {
    set({ personaLoading: true, personaError: null });
    try {
      await api.updatePersona(id, params);
      await get().fetchPersonas();
    } catch (err) {
      set({ personaError: String(err), personaLoading: false });
      throw err;
    }
  },

  deletePersona: async (id) => {
    set({ personaLoading: true, personaError: null });
    try {
      await api.deletePersona(id);
      set((state) => ({
        personas: state.personas.filter((p) => p.id !== id),
        personaLoading: false,
      }));
    } catch (err) {
      set({ personaError: String(err), personaLoading: false });
      throw err;
    }
  },
});
