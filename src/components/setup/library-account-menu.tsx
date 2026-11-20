import { useEffect, useState } from "react"
import { CircleUserRound, X } from "lucide-react"
import { Popover } from "@base-ui/react/popover"
import { useLibraryAccountStore } from "@/stores/library-account-store"
import { useSetupStore } from "@/stores/setup-store"
import { LibraryAccount } from "./library-account"

export function LibraryAccountMenu({ onSignedIn }: { onSignedIn?: () => Promise<void> }) {
  const { username, checked, checking, refresh } = useLibraryAccountStore()
  const [open, setOpen] = useState(false)
  const busy = useSetupStore((s) => s.busy)
  useEffect(() => {
    if (!busy && !checked && !checking) void refresh()
  }, [busy, checked, checking, refresh])

  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger aria-label={username ? `Library account: ${username}` : "Sign in to Library"} title={username ? `Library: ${username}` : "Sign in to Library"} className="relative flex h-10 w-10 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground">
        <CircleUserRound className="h-5 w-5" />
        {username && <span className="absolute right-1 top-1 h-2 w-2 rounded-full bg-emerald-500 ring-2 ring-background" />}
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner side="right" align="end" sideOffset={10} className="z-50">
          <Popover.Popup className="relative max-h-[calc(100dvh-2rem)] w-80 max-w-[calc(100vw-5rem)] overflow-y-auto rounded-xl border bg-popover p-5 text-popover-foreground shadow-xl outline-none">
            <Popover.Title className="sr-only">Library account</Popover.Title>
            <Popover.Close aria-label="Close account menu" className="absolute right-2 top-2 rounded p-1 text-muted-foreground hover:bg-accent"><X className="h-4 w-4" /></Popover.Close>
            <LibraryAccount onSignedIn={onSignedIn && (async () => {
              await onSignedIn()
              setOpen(false)
            })} />
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  )
}
