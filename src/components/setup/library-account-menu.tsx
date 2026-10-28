import { useEffect } from "react"
import { CircleUserRound } from "lucide-react"
import { useLibraryAccountStore } from "@/stores/library-account-store"
import { useSetupStore } from "@/stores/setup-store"
import { useAccountViewStore } from "@/stores/account-view-store"
import { AccountIdentity } from "@/design-system/components"

/** Account credentials belong to a full screen, never a popover. */
export function LibraryAccountMenu() {
  const { username, checked, checking, refresh } = useLibraryAccountStore()
  const busy = useSetupStore((s) => s.busy)
  const setOpen = useAccountViewStore((s) => s.setOpen)
  useEffect(() => {
    if (!busy && !checked && !checking) void refresh()
  }, [busy, checked, checking, refresh])
  return (
    <button
      onClick={() => setOpen(true)}
      aria-label={
        username ? `Library account: ${username}` : "Sign in to Library"
      }
      className="desktop-account-trigger"
    >
      {username ? (
        <AccountIdentity username={username} detail="Library account" />
      ) : (
        <>
          <CircleUserRound className="h-5 w-5" />
          <span>Sign in to Library</span>
        </>
      )}
    </button>
  )
}
