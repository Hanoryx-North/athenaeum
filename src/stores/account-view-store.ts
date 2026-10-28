import { create } from "zustand"

/** Navigation state only; credentials remain in the existing account service. */
export const useAccountViewStore = create<{
  open: boolean
  setOpen: (open: boolean) => void
}>((set) => ({
  open: false,
  setOpen: (open) => set({ open }),
}))
